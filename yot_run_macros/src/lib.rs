use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, LitBool, Token, parse::Parse, parse_macro_input};

struct Args {
    show_ui: bool,
}

impl Parse for Args {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut show_ui = true; // Default value

        if !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            if ident == "show_ui" {
                input.parse::<Token![=]>()?;
                let lit: LitBool = input.parse()?;
                show_ui = lit.value;
            }
        }
        Ok(Args { show_ui })
    }
}

#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as Args);
    let input = parse_macro_input!(item as ItemFn);
    let name = &input.sig.ident;
    let body = &input.block;
    let attrs = &input.attrs;
    let vis = &input.vis;
    let show_ui = args.show_ui;

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
            let runtime = yot_run::runtime::Runtime::new(#show_ui)
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
