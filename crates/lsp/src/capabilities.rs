use tower_lsp::lsp_types::{
    CodeActionOptions, CodeActionProviderCapability, CompletionOptions, DocumentFormattingOptions,
    HoverProviderCapability, InlayHintOptions, InlayHintServerCapabilities, OneOf, RenameOptions,
    ServerCapabilities, SignatureHelpOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
    WorkDoneProgressOptions,
};

#[must_use]
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_owned(), "$".to_owned()]),
            resolve_provider: Some(false),
            ..Default::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
            retrigger_characters: Some(vec![",".to_owned()]),
            work_done_progress_options: Default::default(),
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
            InlayHintOptions { resolve_provider: Some(false), ..Default::default() },
        ))),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        document_formatting_provider: Some(OneOf::Right(DocumentFormattingOptions {
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![tower_lsp::lsp_types::CodeActionKind::QUICKFIX]),
            resolve_provider: Some(false),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        ..Default::default()
    }
}
