use rustc_errors::DiagnosticArgFromDisplay;
use rustc_macros::{LintDiagnostic, SessionDiagnostic, SessionSubdiagnostic};
use rustc_span::{Span, Symbol};

#[derive(SessionDiagnostic)]
#[diag(privacy::field_is_private, code = "E0451")]
pub struct FieldIsPrivate {
    #[primary_span]
    pub span: Span,
    pub field_name: Symbol,
    pub variant_descr: &'static str,
    pub def_path_str: String,
    #[subdiagnostic]
    pub label: FieldIsPrivateLabel,
}

#[derive(SessionSubdiagnostic)]
pub enum FieldIsPrivateLabel {
    #[label(privacy::field_is_private_is_update_syntax_label)]
    IsUpdateSyntax {
        #[primary_span]
        span: Span,
        field_name: Symbol,
    },
    #[label(privacy::field_is_private_label)]
    Other {
        #[primary_span]
        span: Span,
    },
}

#[derive(SessionDiagnostic)]
#[diag(privacy::item_is_private)]
pub struct ItemIsPrivate<'a> {
    #[primary_span]
    #[label]
    pub span: Span,
    pub kind: &'a str,
    pub descr: DiagnosticArgFromDisplay<'a>,
}

#[derive(SessionDiagnostic)]
#[diag(privacy::unnamed_item_is_private)]
pub struct UnnamedItemIsPrivate {
    #[primary_span]
    pub span: Span,
    pub kind: &'static str,
}

// Duplicate of `InPublicInterface` but with a different error code, shares the same slug.
#[derive(SessionDiagnostic)]
#[diag(privacy::in_public_interface, code = "E0445")]
pub struct InPublicInterfaceTraits<'a> {
    #[primary_span]
    #[label]
    pub span: Span,
    pub vis_descr: &'static str,
    pub kind: &'a str,
    pub descr: DiagnosticArgFromDisplay<'a>,
    #[label(privacy::visibility_label)]
    pub vis_span: Span,
}

// Duplicate of `InPublicInterfaceTraits` but with a different error code, shares the same slug.
#[derive(SessionDiagnostic)]
#[diag(privacy::in_public_interface, code = "E0446")]
pub struct InPublicInterface<'a> {
    #[primary_span]
    #[label]
    pub span: Span,
    pub vis_descr: &'static str,
    pub kind: &'a str,
    pub descr: DiagnosticArgFromDisplay<'a>,
    #[label(privacy::visibility_label)]
    pub vis_span: Span,
}

#[derive(LintDiagnostic)]
#[diag(privacy::from_private_dep_in_public_interface)]
pub struct FromPrivateDependencyInPublicInterface<'a> {
    pub kind: &'a str,
    pub descr: DiagnosticArgFromDisplay<'a>,
    pub krate: Symbol,
}

#[derive(LintDiagnostic)]
#[diag(privacy::private_in_public_lint)]
pub struct PrivateInPublicLint<'a> {
    pub vis_descr: &'static str,
    pub kind: &'a str,
    pub descr: DiagnosticArgFromDisplay<'a>,
}
