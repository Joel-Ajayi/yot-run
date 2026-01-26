use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let name = &input.sig.ident;
    let body = &input.block;
    let attrs = &input.attrs;
    let vis = &input.vis;

    // Ensure the function is async
    if input.sig.asyncness.is_none() {
        return quote! { compile_error!("The #[yot_run::main] function must be async"); }.into();
    }

    if name != "main" {
        return quote! {
            compile_error!("#[yot_run::main] can only be applied to the 'main' function");
        }
        .into();
    }

    let result = quote! {
        #(#attrs)*
        #vis fn main() {
            // 1. Bootstrap the Runtime
            let runtime = yot_run::runtime::Runtime::new()
                .expect("Failed to initialize runtime");

            // 2. Block on the user's main body
            runtime.block_on(async {
                #body
            });

            // The program only reaches this point once the async block above is done.
        }
    };
    result.into()
}
