use std::sync::Arc;

use gdscript_api_db::ApiDb;
use gdscript_indexer::{ProjectIndex, index::index_workspace};
use tokio::sync::RwLock;
use tower_lsp::lsp_types::{
    CodeActionOrCommand, CodeActionParams, CompletionParams, CompletionResponse, Diagnostic,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    DidChangeWatchedFilesParams, FileSystemWatcher, GlobPattern, InitializeParams, InitializeResult,
    InlayHint, InlayHintParams, Location, MessageType, Position, PrepareRenameResponse, Range,
    ReferenceParams, Registration, RenameParams, ServerInfo, SignatureHelp, SignatureHelpParams,
    SymbolInformation, SymbolKind, TextEdit, WatchKind, WorkspaceEdit, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer, jsonrpc::Result};

use crate::{
    call_checker::check_calls,
    capabilities::server_capabilities,
    code_actions::code_actions_for,
    completion::{class_name_completions, member_completions, node_member_completions, node_name_completions},
    diagnostics::publish_diagnostics,
    document_store::DocumentStore,
    formatting::format_document,
    goto_def::find_definition,
    hover::hover_at as hover_for_word,
    inlay_hints::inlay_hints,
    signature_help::signature_help_for_method,
    text_util::word_at,
    type_check::check_type_mismatches,
    type_resolver::{TypeMap, extract_types},
};

fn to_lsp_symbol_kind(kind: &gdscript_core::symbol::SymbolKind) -> SymbolKind {
    match kind {
        gdscript_core::symbol::SymbolKind::Function => SymbolKind::FUNCTION,
        gdscript_core::symbol::SymbolKind::Variable => SymbolKind::VARIABLE,
        gdscript_core::symbol::SymbolKind::Constant => SymbolKind::CONSTANT,
        gdscript_core::symbol::SymbolKind::Signal => SymbolKind::EVENT,
        gdscript_core::symbol::SymbolKind::Class => SymbolKind::CLASS,
        gdscript_core::symbol::SymbolKind::Enum => SymbolKind::ENUM,
        gdscript_core::symbol::SymbolKind::EnumMember => SymbolKind::ENUM_MEMBER,
    }
}

pub struct Backend {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
    api_db: Arc<RwLock<Option<ApiDb>>>,
    project_index: Arc<RwLock<ProjectIndex>>,
    workspace_root: Arc<RwLock<Option<std::path::PathBuf>>>,
    /// Per-file type maps, rebuilt on every open/change.
    type_maps: Arc<RwLock<std::collections::HashMap<tower_lsp::lsp_types::Url, TypeMap>>>,
}

impl Backend {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::default())),
            api_db: Arc::new(RwLock::new(None)),
            project_index: Arc::new(RwLock::new(ProjectIndex::new())),
            workspace_root: Arc::new(RwLock::new(None)),
            type_maps: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    async fn update_type_map(&self, uri: &tower_lsp::lsp_types::Url, source: &str) {
        if let Ok(doc) = gdscript_parser::parse::parse(source) {
            let map = extract_types(&doc);
            self.type_maps.write().await.insert(uri.clone(), map);
        }
    }

    async fn run_call_checker(
        &self,
        uri: &tower_lsp::lsp_types::Url,
        source: &str,
    ) -> Vec<tower_lsp::lsp_types::Diagnostic> {
        let Ok(doc) = gdscript_parser::parse::parse(source) else {
            return vec![];
        };
        let db = self.api_db.read().await;
        let Some(db) = db.as_ref() else { return vec![] };
        let type_maps = self.type_maps.read().await;
        let empty = TypeMap::default();
        let type_map = type_maps.get(uri).unwrap_or(&empty);
        let mut diags = check_calls(&doc, type_map, db);
        diags.extend(check_type_mismatches(&doc, db));
        diags
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Prefer root_uri; fall back to first workspace folder.
        let root_path = params
            .root_uri
            .and_then(|u| u.to_file_path().ok())
            .or_else(|| {
                params
                    .workspace_folders
                    .as_deref()
                    .and_then(|folders| folders.first())
                    .and_then(|f| f.uri.to_file_path().ok())
            });

        if let Some(path) = root_path {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("gdscript-lsp: workspace root = {}", path.display()),
                )
                .await;
            *self.workspace_root.write().await = Some(path);
        } else {
            self.client
                .log_message(
                    MessageType::WARNING,
                    "gdscript-lsp: no workspace root from initialize — will try to infer from first opened file",
                )
                .await;
        }

        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(ServerInfo {
                name: "gdscript-lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        })
    }

    async fn initialized(&self, _: tower_lsp::lsp_types::InitializedParams) {
        match ApiDb::bundled() {
            Ok(db) => {
                let class_count = db.class_names().count();
                *self.api_db.write().await = Some(db);
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("gdscript-lsp initialized ({class_count} engine classes loaded)"),
                    )
                    .await;
            }
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("failed to load engine API database: {e}"),
                    )
                    .await;
            }
        }

        let root = self.workspace_root.read().await.clone();
        if let Some(root) = root {
            let index = self.project_index.clone();
            let client = self.client.clone();
            tokio::spawn(async move {
                match index_workspace(&root) {
                    Ok(new_index) => {
                        let class_count = new_index.class_names.len();
                        let autoload_count = new_index.autoloads.len();
                        let file_count = new_index.file_symbols.len();
                        *index.write().await = new_index;
                        client
                            .log_message(
                                MessageType::INFO,
                                format!(
                                    "project indexed: {file_count} files, {class_count} named classes, {autoload_count} autoloads"
                                ),
                            )
                            .await;
                    }
                    Err(e) => {
                        client
                            .log_message(
                                MessageType::WARNING,
                                format!("project indexing failed: {e}"),
                            )
                            .await;
                    }
                }

                if let Err(e) = gdscript_indexer::watcher::watch(&root, index) {
                    client
                        .log_message(
                            MessageType::WARNING,
                            format!("file watcher failed to start: {e}"),
                        )
                        .await;
                }

                // Ask the client to watch project files so we get notified
                // via workspace/didChangeWatchedFiles.
                let patterns = ["**/*.gd", "**/*.tscn", "**/project.godot"];
                let watchers: Vec<FileSystemWatcher> = patterns
                    .iter()
                    .map(|glob| FileSystemWatcher {
                        glob_pattern: GlobPattern::String((*glob).to_owned()),
                        kind: Some(WatchKind::all()),
                    })
                    .collect();
                let _ = client
                    .register_capability(vec![Registration {
                        id: "workspace/didChangeWatchedFiles".to_owned(),
                        method: "workspace/didChangeWatchedFiles".to_owned(),
                        register_options: serde_json::to_value(
                            tower_lsp::lsp_types::DidChangeWatchedFilesRegistrationOptions {
                                watchers,
                            },
                        )
                        .ok(),
                    }])
                    .await;
            });
        }
    }

    async fn did_change_watched_files(&self, _params: DidChangeWatchedFilesParams) {
        let root = self.workspace_root.read().await.clone();
        let Some(root) = root else { return };

        let index = self.project_index.clone();
        tokio::spawn(async move {
            if let Ok(new_index) = index_workspace(&root) {
                *index.write().await = new_index;
            }
        });
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;
        let text = params.text_document.text.clone();

        self.documents
            .write()
            .await
            .open(uri.clone(), params.text_document.text);

        // If workspace root was never set, try to infer it from this file by
        // walking up to find project.godot. This handles clients that don't
        // send root_uri / workspaceFolders in the initialize request.
        {
            let needs_root = self.workspace_root.read().await.is_none();
            if needs_root {
                if let Some(file_path) = uri.to_file_path().ok() {
                    if let Some(root) = find_project_root(&file_path) {
                        self.client
                            .log_message(
                                MessageType::INFO,
                                format!("gdscript-lsp: inferred workspace root = {}", root.display()),
                            )
                            .await;
                        *self.workspace_root.write().await = Some(root.clone());
                        let index = self.project_index.clone();
                        let client = self.client.clone();
                        tokio::spawn(async move {
                            match index_workspace(&root) {
                                Ok(new_index) => {
                                    let class_count = new_index.class_names.len();
                                    let autoload_count = new_index.autoloads.len();
                                    let file_count = new_index.file_symbols.len();
                                    *index.write().await = new_index;
                                    client
                                        .log_message(
                                            MessageType::INFO,
                                            format!(
                                                "project indexed (deferred): {file_count} files, {class_count} named classes, {autoload_count} autoloads"
                                            ),
                                        )
                                        .await;
                                }
                                Err(e) => {
                                    client
                                        .log_message(
                                            MessageType::WARNING,
                                            format!("deferred project indexing failed: {e}"),
                                        )
                                        .await;
                                }
                            }
                        });
                    }
                }
            }
        }

        self.update_type_map(&uri, &text).await;
        let call_diags = self.run_call_checker(&uri, &text).await;
        publish_diagnostics(&self.client, uri, version, &text, call_diags).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };

        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;
        let text = change.text.clone();

        self.documents
            .write()
            .await
            .update(&params.text_document.uri, change.text);

        self.update_type_map(&uri, &text).await;
        let call_diags = self.run_call_checker(&uri, &text).await;
        publish_diagnostics(&self.client, uri, version, &text, call_diags).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.client
            .publish_diagnostics(params.text_document.uri.clone(), vec![], None)
            .await;
        self.documents
            .write()
            .await
            .close(&params.text_document.uri);
        self.type_maps.write().await.remove(&params.text_document.uri);
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {}

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = &params.text_document_position_params.position;
        let uri = &params.text_document_position_params.text_document.uri;

        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else {
            return Ok(None);
        };

        let word = word_at(&source, pos.line, pos.character);
        let Some(word) = word else {
            return Ok(None);
        };

        let lines: Vec<&str> = source.lines().collect();
        let line = lines.get(pos.line as usize).copied().unwrap_or("");
        let char_pos = pos.character as usize;

        let db = self.api_db.read().await;
        let Some(db) = db.as_ref() else {
            return Ok(None);
        };

        let type_maps = self.type_maps.read().await;
        let empty = TypeMap::default();
        let type_map = type_maps.get(uri).unwrap_or(&empty);

        Ok(hover_for_word(word, line, char_pos, type_map, db))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let trigger = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.as_deref());

        let uri = &params.text_document_position.text_document.uri;
        let pos = &params.text_document_position.position;

        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else { return Ok(None) };

        let lines: Vec<&str> = source.lines().collect();
        let line = lines.get(pos.line as usize).copied().unwrap_or("");
        let char_pos = pos.character as usize;
        let _before: String = line.chars().take(char_pos).collect();

        // `$` trigger — show node names from the associated scene.
        if trigger == Some("$") {
            let script_path = uri.to_file_path().ok();
            let index = self.project_index.read().await;
            if let Some(script_path) = script_path {
                if let Some(scene_map) = find_associated_scene(&script_path, &index) {
                    return Ok(Some(node_name_completions(scene_map)));
                }
            }
            return Ok(None);
        }

        if trigger == Some(".") {
            // Walk back on the current line to find the identifier before `.`
            let char_pos_before_dot = char_pos.saturating_sub(1);
            let before_dot: String = line.chars().take(char_pos_before_dot).collect();

            // Check for `$NodeName.` pattern.
            if let Some(dollar_pos) = before_dot.rfind('$') {
                let node_path: String = before_dot[dollar_pos + 1..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '/')
                    .collect();
                if !node_path.is_empty() {
                    let script_path = uri.to_file_path().ok();
                    let index = self.project_index.read().await;
                    let db = self.api_db.read().await;
                    if let (Some(script_path), Some(db)) = (script_path, db.as_ref()) {
                        if let Some(scene_map) = find_associated_scene(&script_path, &index) {
                            let result = node_member_completions(&node_path, scene_map, db);
                            if result.is_some() {
                                return Ok(result);
                            }
                        }
                    }
                }
            }

            let receiver_owned = before_dot
                .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let Some(receiver) = receiver_owned.as_deref() else {
                return Ok(None);
            };

            let db = self.api_db.read().await;
            let Some(db) = db.as_ref() else { return Ok(None) };
            let type_maps = self.type_maps.read().await;
            let empty = TypeMap::default();
            let type_map = type_maps.get(uri).unwrap_or(&empty);

            // Resolve the receiver's type — type map → engine class → user
            // class_name → autoload singleton.
            let index = self.project_index.read().await;
            let resolved_type = type_map.resolve(receiver)
                .map(str::to_owned)
                .or_else(|| db.get_class(receiver).map(|c| c.name.clone()))
                .or_else(|| index.class_names.contains_key(receiver).then(|| receiver.to_owned()))
                .or_else(|| index.autoloads.contains_key(receiver).then(|| receiver.to_owned()));

            self.client.log_message(
                MessageType::INFO,
                format!("dot completion: receiver={receiver:?} resolved={resolved_type:?}"),
            ).await;

            let result = if let Some(ref type_name) = resolved_type {
                if db.get_class(type_name).is_some() {
                    // Engine type — full member completions.
                    // For `self.`, also inject the current file's own symbols so
                    // user-defined methods and signals appear alongside engine members.
                    let mut fake_map = TypeMap::default();
                    fake_map.types.insert(receiver.to_owned(), type_name.clone());
                    let mut base = member_completions(receiver, &fake_map, db);
                    if receiver == "self" {
                        if let Some(script_path) = uri.to_file_path().ok() {
                            if let Some(own) = user_class_completions_by_path(&script_path, &index) {
                                base = Some(merge_completions(base, own));
                            }
                        }
                    }
                    base
                } else {
                    // User class or autoload — show its file's symbols.
                    let result = user_class_completions(type_name, &index);
                    self.client.log_message(
                        MessageType::INFO,
                        format!(
                            "dot completion: user_class_completions({type_name}) → {} items",
                            result.as_ref().map(|r| match r {
                                tower_lsp::lsp_types::CompletionResponse::Array(v) => v.len(),
                                _ => 0,
                            }).unwrap_or(0)
                        ),
                    ).await;
                    result
                }
            } else {
                member_completions(receiver, type_map, db)
            };

            return Ok(result);
        }

        // No trigger — return class names + current file's own members +
        // base-class engine members (GDScript: `self.` is implicit).
        let db = self.api_db.read().await;
        let type_maps = self.type_maps.read().await;
        let empty = TypeMap::default();
        let type_map = type_maps.get(uri).unwrap_or(&empty);
        let index = self.project_index.read().await;

        let Some(db) = db.as_ref() else { return Ok(None) };

        // All class names (engine + user + autoloads)
        let mut all = match class_name_completions(db, &index.class_names, &index.autoloads) {
            tower_lsp::lsp_types::CompletionResponse::Array(v) => v,
            _ => vec![],
        };

        // Current file's own symbols (methods, vars, signals, etc.)
        if let Some(script_path) = uri.to_file_path().ok() {
            if let Some(own) = user_class_completions_by_path(&script_path, &index) {
                if let tower_lsp::lsp_types::CompletionResponse::Array(items) = own {
                    let seen: std::collections::HashSet<_> = all.iter().map(|i| i.label.clone()).collect();
                    for item in items {
                        if !seen.contains(&item.label) {
                            all.push(item);
                        }
                    }
                }
            }
        }

        // Base class engine members (implicit self)
        if let Some(self_type) = type_map.self_type.as_deref() {
            if let Some(base) = member_completions("self", type_map, db) {
                if let tower_lsp::lsp_types::CompletionResponse::Array(items) = base {
                    let seen: std::collections::HashSet<_> = all.iter().map(|i| i.label.clone()).collect();
                    for item in items {
                        if !seen.contains(&item.label) {
                            all.push(item);
                        }
                    }
                }
            }
            let _ = self_type; // used above via type_map
        }

        Ok(Some(tower_lsp::lsp_types::CompletionResponse::Array(all)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = &params.text_document_position_params.position;

        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else {
            return Ok(None);
        };

        let lines: Vec<&str> = source.lines().collect();
        let line = lines.get(pos.line as usize).copied().unwrap_or("");
        let before = &line[..pos.character as usize];

        // Parse `receiver.method(` pattern
        let Some((receiver, method)) = parse_call_context(before) else {
            return Ok(None);
        };

        // Count commas to find active parameter
        let after_open = before.rfind('(').map_or("", |i| &before[i + 1..]);
        let active_param = after_open.chars().filter(|&c| c == ',').count() as u32;

        let db = self.api_db.read().await;
        let Some(db) = db.as_ref() else {
            return Ok(None);
        };

        let type_maps = self.type_maps.read().await;
        let empty = TypeMap::default();
        let type_map = type_maps.get(uri).unwrap_or(&empty);

        // Handle direct class name (e.g. `Node2D.new(`)
        let result = if db.get_class(receiver).is_some() {
            let mut fake_map = TypeMap::default();
            fake_map.types.insert(receiver.to_owned(), receiver.to_owned());
            signature_help_for_method(receiver, method, active_param, &fake_map, db)
        } else {
            signature_help_for_method(receiver, method, active_param, type_map, db)
        };

        Ok(result)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = &params.text_document_position_params.position;
        let uri = &params.text_document_position_params.text_document.uri;

        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else {
            return Ok(None);
        };

        let word = word_at(&source, pos.line, pos.character);
        let Some(word) = word else {
            return Ok(None);
        };

        // Check project index: class_name declarations and autoloads.
        let index = self.project_index.read().await;
        let index_path = index.class_names.get(word).or_else(|| index.autoloads.get(word));
        if let Some(path) = index_path {
            if let Ok(target_uri) = tower_lsp::lsp_types::Url::from_file_path(path) {
                return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: target_uri,
                    range: Range::default(),
                })));
            }
        }
        drop(index);

        // Fall back to same-file definition.
        let Ok(doc) = gdscript_parser::parse::parse(&source) else {
            return Ok(None);
        };

        let location = find_definition(&doc, uri, word);
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else {
            return Ok(None);
        };
        let Ok(doc) = gdscript_parser::parse::parse(&source) else {
            return Ok(None);
        };
        let symbols = gdscript_parser::symbol_table::extract_symbols(&doc);
        #[allow(deprecated)]
        let result = symbols
            .into_iter()
            .map(|s| SymbolInformation {
                name: s.name,
                kind: to_lsp_symbol_kind(&s.kind),
                location: tower_lsp::lsp_types::Location {
                    uri: uri.clone(),
                    range: tower_lsp::lsp_types::Range {
                        start: tower_lsp::lsp_types::Position {
                            line: s.line,
                            character: s.col,
                        },
                        end: tower_lsp::lsp_types::Position {
                            line: s.line,
                            character: s.col,
                        },
                    },
                },
                tags: None,
                deprecated: None,
                container_name: None,
            })
            .collect();
        Ok(Some(DocumentSymbolResponse::Flat(result)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = params.query.to_lowercase();
        let query = query.as_str();
        let index = self.project_index.read().await;

        #[allow(deprecated)]
        let results: Vec<SymbolInformation> = index
            .file_symbols
            .iter()
            .flat_map(|(path, symbols)| {
                let uri = tower_lsp::lsp_types::Url::from_file_path(path).ok();
                symbols.iter().filter_map(move |s| {
                    if !query.is_empty() && !s.name.to_lowercase().contains(&query) {
                        return None;
                    }
                    let uri = uri.clone()?;
                    Some(SymbolInformation {
                        name: s.name.clone(),
                        kind: to_lsp_symbol_kind(&s.kind),
                        location: Location {
                            uri,
                            range: Range {
                                start: Position { line: s.line, character: s.col },
                                end: Position { line: s.line, character: s.col },
                            },
                        },
                        tags: None,
                        deprecated: None,
                        container_name: None,
                    })
                })
            })
            .collect();

        Ok(if results.is_empty() { None } else { Some(results) })
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = &params.text_document_position.position;

        let documents = self.documents.read().await;
        let source = documents.get(uri).map(str::to_owned);
        let Some(source) = source else { return Ok(None) };

        let Some(word) = word_at(&source, pos.line, pos.character) else {
            return Ok(None);
        };
        let word = word.to_owned();

        // Build search corpus: indexed files on disk + all open documents
        // (open docs take priority over disk — they may have unsaved changes).
        let index = self.project_index.read().await;
        let indexed_paths: Vec<std::path::PathBuf> =
            index.file_symbols.keys().cloned().collect();

        // Collect open documents keyed by their URI.
        let open_docs: std::collections::HashMap<tower_lsp::lsp_types::Url, String> =
            documents.all();
        drop(index);
        drop(documents);

        let mut locations = Vec::new();

        // Search indexed files, preferring in-memory content for open docs.
        for path in &indexed_paths {
            let file_uri = match tower_lsp::lsp_types::Url::from_file_path(path) {
                Ok(u) => u,
                Err(_) => continue,
            };
            let text = if let Some(doc) = open_docs.get(&file_uri) {
                doc.clone()
            } else {
                match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(_) => continue,
                }
            };
            search_word_in_text(&text, &word, &file_uri, &mut locations);
        }

        // Also search any open document not in the index (e.g. new unsaved files).
        for (doc_uri, text) in &open_docs {
            let already_searched = doc_uri
                .to_file_path()
                .ok()
                .map(|p| indexed_paths.contains(&p))
                .unwrap_or(false);
            if !already_searched {
                search_word_in_text(text, &word, doc_uri, &mut locations);
            }
        }

        Ok(if locations.is_empty() { None } else { Some(locations) })
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else { return Ok(None) };

        let Ok(doc) = gdscript_parser::parse::parse(&source) else {
            return Ok(None);
        };

        let hints = inlay_hints(&doc, &params.range);
        Ok(if hints.is_empty() { None } else { Some(hints) })
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else { return Ok(None) };

        let gdformat_path = "gdformat";
        let edits = format_document(&source, gdformat_path).await;
        Ok(edits)
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<Vec<CodeActionOrCommand>>> {
        let uri = &params.text_document.uri;
        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else { return Ok(None) };

        let diags: Vec<Diagnostic> = params.context.diagnostics;
        let actions = code_actions_for(uri, &source, &params.range, &diags);
        Ok(if actions.is_empty() { None } else { Some(actions) })
    }

    async fn prepare_rename(
        &self,
        params: tower_lsp::lsp_types::TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else { return Ok(None) };

        let Some(word) = word_at(&source, params.position.line, params.position.character) else {
            return Ok(None);
        };

        // Refuse to rename engine built-ins.
        let db = self.api_db.read().await;
        if let Some(db) = db.as_ref() {
            if db.get_class(word).is_some() {
                return Ok(None);
            }
        }

        // Find the byte range of the word on that line.
        let line_text = source
            .lines()
            .nth(params.position.line as usize)
            .unwrap_or("");
        let col = params.position.character as usize;
        // Walk backwards from col to find where the word starts.
        let prefix: String = line_text.chars().take(col + 1).collect();
        let start_char = prefix
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
            .map_or(0, |(i, c)| i + c.len_utf8());
        let end_char = start_char + word.len();

        Ok(Some(PrepareRenameResponse::Range(Range {
            start: Position { line: params.position.line, character: start_char as u32 },
            end: Position { line: params.position.line, character: end_char as u32 },
        })))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = &params.text_document_position.position;
        let new_name = &params.new_name;

        let source = self.documents.read().await.get(uri).map(str::to_owned);
        let Some(source) = source else { return Ok(None) };

        let Some(word) = word_at(&source, pos.line, pos.character) else {
            return Ok(None);
        };
        let word = word.to_owned();

        let index = self.project_index.read().await;
        let paths: Vec<std::path::PathBuf> = index.file_symbols.keys().cloned().collect();
        drop(index);

        let mut changes: std::collections::HashMap<
            tower_lsp::lsp_types::Url,
            Vec<TextEdit>,
        > = std::collections::HashMap::new();

        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Ok(file_uri) = tower_lsp::lsp_types::Url::from_file_path(&path) else { continue };

            let mut edits: Vec<TextEdit> = Vec::new();
            for (line_num, line_text) in text.lines().enumerate() {
                let mut search = line_text;
                let mut col_offset = 0usize;
                while let Some(pos) = search.find(word.as_str()) {
                    let abs = col_offset + pos;
                    let before_ok = abs == 0
                        || !line_text.as_bytes()[abs - 1].is_ascii_alphanumeric()
                            && line_text.as_bytes()[abs - 1] != b'_';
                    let after = abs + word.len();
                    let after_ok = after >= line_text.len()
                        || !line_text.as_bytes()[after].is_ascii_alphanumeric()
                            && line_text.as_bytes()[after] != b'_';

                    if before_ok && after_ok {
                        edits.push(TextEdit {
                            range: Range {
                                start: Position { line: line_num as u32, character: abs as u32 },
                                end: Position { line: line_num as u32, character: (abs + word.len()) as u32 },
                            },
                            new_text: new_name.clone(),
                        });
                    }

                    col_offset += pos + 1;
                    search = &search[pos + 1..];
                }
            }

            if !edits.is_empty() {
                changes.insert(file_uri, edits);
            }
        }

        if changes.is_empty() {
            return Ok(None);
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }
}

/// Build completions for a user-defined class or autoload singleton by looking
/// up its symbols in the project index's `file_symbols` map.
/// Look up symbols directly by file path (for `self.` completions on the current file).
fn user_class_completions_by_path(
    path: &std::path::Path,
    index: &gdscript_indexer::ProjectIndex,
) -> Option<tower_lsp::lsp_types::CompletionResponse> {
    use gdscript_core::symbol::SymbolKind;
    use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse};

    let symbols = index.file_symbols.get(path)?;
    let items: Vec<CompletionItem> = symbols
        .iter()
        .map(|sym| {
            let kind = match sym.kind {
                SymbolKind::Function => CompletionItemKind::FUNCTION,
                SymbolKind::Variable => CompletionItemKind::FIELD,
                SymbolKind::Constant => CompletionItemKind::CONSTANT,
                SymbolKind::Signal => CompletionItemKind::EVENT,
                SymbolKind::Class => CompletionItemKind::CLASS,
                SymbolKind::Enum | SymbolKind::EnumMember => CompletionItemKind::ENUM,
            };
            CompletionItem { label: sym.name.clone(), kind: Some(kind), ..Default::default() }
        })
        .collect();
    if items.is_empty() { None } else { Some(CompletionResponse::Array(items)) }
}

/// Merge two CompletionResponses, deduplicating by label.
fn merge_completions(
    a: Option<tower_lsp::lsp_types::CompletionResponse>,
    b: tower_lsp::lsp_types::CompletionResponse,
) -> tower_lsp::lsp_types::CompletionResponse {
    use tower_lsp::lsp_types::CompletionResponse;
    use std::collections::HashSet;

    let mut items = match a {
        Some(CompletionResponse::Array(v)) => v,
        _ => Vec::new(),
    };
    let seen: HashSet<String> = items.iter().map(|i| i.label.clone()).collect();
    if let CompletionResponse::Array(extra) = b {
        for item in extra {
            if !seen.contains(&item.label) {
                items.push(item);
            }
        }
    }
    CompletionResponse::Array(items)
}

fn user_class_completions(
    class_name: &str,
    index: &gdscript_indexer::ProjectIndex,
) -> Option<tower_lsp::lsp_types::CompletionResponse> {
    use gdscript_core::symbol::SymbolKind;
    use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse};

    // Try class_names first, then autoloads
    let path = index
        .class_names
        .get(class_name)
        .or_else(|| index.autoloads.get(class_name))?;
    let symbols = index.file_symbols.get(path)?;

    let items: Vec<CompletionItem> = symbols
        .iter()
        .map(|sym| {
            let kind = match sym.kind {
                SymbolKind::Function => CompletionItemKind::FUNCTION,
                SymbolKind::Variable => CompletionItemKind::FIELD,
                SymbolKind::Constant => CompletionItemKind::CONSTANT,
                SymbolKind::Signal => CompletionItemKind::EVENT,
                SymbolKind::Class => CompletionItemKind::CLASS,
                SymbolKind::Enum | SymbolKind::EnumMember => CompletionItemKind::ENUM,
            };
            CompletionItem {
                label: sym.name.clone(),
                kind: Some(kind),
                detail: sym.type_annotation.clone().or_else(|| Some(class_name.to_owned())),
                ..Default::default()
            }
        })
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

/// Walk up from `file_path` to find the nearest directory containing `project.godot`.
fn find_project_root(file_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = file_path.parent()?;
    loop {
        if dir.join("project.godot").exists() {
            return Some(dir.to_owned());
        }
        dir = dir.parent()?;
    }
}

/// Scan `text` for whole-word occurrences of `word` and append matches to `out`.
fn search_word_in_text(
    text: &str,
    word: &str,
    uri: &tower_lsp::lsp_types::Url,
    out: &mut Vec<Location>,
) {
    for (line_num, line_text) in text.lines().enumerate() {
        let bytes = line_text.as_bytes();
        let wlen = word.len();
        let mut start = 0usize;
        while start + wlen <= bytes.len() {
            if let Some(rel) = line_text[start..].find(word) {
                let abs = start + rel;
                let before_ok = abs == 0
                    || (!bytes[abs - 1].is_ascii_alphanumeric() && bytes[abs - 1] != b'_');
                let after = abs + wlen;
                let after_ok = after >= bytes.len()
                    || (!bytes[after].is_ascii_alphanumeric() && bytes[after] != b'_');
                if before_ok && after_ok {
                    out.push(Location {
                        uri: uri.clone(),
                        range: Range {
                            start: Position { line: line_num as u32, character: abs as u32 },
                            end: Position { line: line_num as u32, character: after as u32 },
                        },
                    });
                }
                start = abs + 1;
            } else {
                break;
            }
        }
    }
}

/// Find the scene node map associated with a `.gd` script by looking for a
/// same-name `.tscn` file in the same directory (Godot's naming convention).
fn find_associated_scene<'a>(
    script_path: &std::path::Path,
    index: &'a gdscript_indexer::ProjectIndex,
) -> Option<&'a gdscript_indexer::scene::SceneNodeMap> {
    let stem = script_path.file_stem()?.to_str()?;
    let dir = script_path.parent()?;
    let tscn = dir.join(format!("{stem}.tscn"));
    index.scenes.get(&tscn)
}

/// Parse `receiver.method(` from a line prefix, returning `(receiver, method)`.
fn parse_call_context(before: &str) -> Option<(&str, &str)> {
    let open = before.rfind('(')?;
    let before_open = before[..open].trim_end();
    let dot = before_open.rfind('.')?;
    let method = before_open[dot + 1..].trim();
    let receiver_end = dot;
    let receiver_start = before_open[..receiver_end]
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map_or(0, |i| i + 1);
    let receiver = &before_open[receiver_start..receiver_end];
    if receiver.is_empty() || method.is_empty() {
        return None;
    }
    Some((receiver, method))
}
