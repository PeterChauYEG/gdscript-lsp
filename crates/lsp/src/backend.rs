use std::sync::Arc;

use gdscript_api_db::ApiDb;
use tokio::sync::RwLock;
use tower_lsp::lsp_types::{
    CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, InitializeParams, InitializeResult, Location,
    MessageType, ReferenceParams, ServerInfo,
};
use tower_lsp::{Client, LanguageServer, jsonrpc::Result};

use crate::{
    capabilities::server_capabilities, completion::class_name_completions,
    diagnostics::publish_syntax_diagnostics, document_store::DocumentStore,
    goto_def::find_definition, hover::hover_for_word, text_util::word_at,
};

pub struct Backend {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
    api_db: Arc<RwLock<Option<ApiDb>>>,
}

impl Backend {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::default())),
            api_db: Arc::new(RwLock::new(None)),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
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

        publish_syntax_diagnostics(&self.client, uri, version, &text).await;
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

        publish_syntax_diagnostics(&self.client, uri, version, &text).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        // Clear diagnostics when the file is closed.
        self.client
            .publish_diagnostics(params.text_document.uri.clone(), vec![], None)
            .await;
        self.documents
            .write()
            .await
            .close(&params.text_document.uri);
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

        let db = self.api_db.read().await;
        let result = db.as_ref().and_then(|db| hover_for_word(word, db));
        Ok(result)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        // Skip member-access completions (after `.`) until type inference is implemented.
        if params.context.and_then(|c| c.trigger_character).as_deref() == Some(".") {
            return Ok(None);
        }

        let db = self.api_db.read().await;
        let result = db.as_ref().map(class_name_completions);
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

        let Ok(doc) = gdscript_parser::parse::parse(&source) else {
            return Ok(None);
        };

        let location = find_definition(&doc, uri, word);
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn references(&self, _params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        // TODO(LAB-666): find references
        Ok(None)
    }
}
