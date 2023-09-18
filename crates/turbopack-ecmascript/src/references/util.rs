use anyhow::Result;
use swc_core::quote;
use swc_ecma_ast::Expr;
use turbo_tasks::Vc;
use turbopack_core::resolve::parse::Request;

/// Creates a IIFE expression that throws a "Cannot find module" error for the
/// given request string
pub fn throw_module_not_found_expr(request: &str) -> Expr {
    let message = format!("Cannot find module '{request}'");
    quote!(
        "(() => { const e = new Error($message); e.code = 'MODULE_NOT_FOUND'; throw e; })()"
            as Expr,
        message: Expr = message.into()
    )
}

#[turbo_tasks::function]
pub async fn request_to_string(request: Vc<Request>) -> Result<Vc<String>> {
    Ok(Vc::cell(
        request
            .await?
            .request()
            // TODO: Handle Request::Dynamic, Request::Alternatives
            .unwrap_or_else(|| "unknown".into()),
    ))
}
