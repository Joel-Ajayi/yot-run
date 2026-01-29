//! Procedural macros for the `yot_run` async runtime.
//!
//! This crate provides the `#[main]` procedural macro attribute that simplifies
//! setting up and running async code in the `yot_run` runtime.
//!
//! # Features
//!
//! - **Automatic Runtime Setup**: The `#[main]` macro automatically initializes
//!   and manages the `yot_run` runtime, so you don't have to do it manually.
//! - **Configurable UI**: Control whether the runtime displays a UI using the
//!   `show_ui` parameter.
//! - **Async Main Function**: Allows you to write async code directly in your
//!   main function without manual boilerplate.
//!
//! # Examples
//!
//! Basic usage with the UI enabled (default):
//!
//! ```ignore
//! #[yot_run::main]
//! async fn main() {
//!     println!("Hello from async main!");
//! }
//! ```
//!
//! Disable the UI:
//!
//! ```ignore
//! #[yot_run::main(show_ui = false)]
//! async fn main() {
//!     println!("Running without UI");
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, LitBool, Token, parse::Parse, parse_macro_input};

/// Configuration arguments for the `#[main]` macro.
///
/// # Fields
///
/// * `show_ui` - Boolean flag to control whether the runtime UI should be displayed.
///   Defaults to `true` if not specified.
struct Args {
    show_ui: bool,
}

impl Parse for Args {
    /// Parses the macro arguments from the attribute input.
    ///
    /// Expected format: `show_ui = <bool>`
    ///
    /// # Arguments
    ///
    /// * `input` - The token stream containing the macro arguments
    ///
    /// # Returns
    ///
    /// A new `Args` instance with parsed or default values.
    ///
    /// # Errors
    ///
    /// Returns a parse error if the input syntax is invalid.
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
/// Procedural macro attribute to set up an async main function with `yot_run` runtime.
///
/// This macro transforms an async main function into a synchronous one that bootstraps
/// the `yot_run` runtime and executes the async code within it.
///
/// # Attributes
///
/// - `show_ui` (optional): A boolean flag to control runtime UI visibility. Defaults to `true`.
///
/// # Requirements
///
/// - The annotated function must be named `main`
/// - The annotated function must be `async`
///
/// # Returns
///
/// Expands to synchronous main function code that:
/// 1. Initializes the `yot_run` runtime
/// 2. Executes the async function body within the runtime
/// 3. Blocks until completion
///
/// # Errors
///
/// Compile errors will occur if:
/// - The function is not declared as `async`
/// - The function is not named `main`
///
/// # Example
///
/// ```rust
/// use yot_run;
///
/// #[yot_run::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let listener = yot_run::net::TcpListener::bind("127.0.0.1:8080").await?;
///     println!("Server listening on 127.0.0.1:8080");
///
///     loop {
///         // 1. Asynchronously wait for a new connection
///         let (mut socket, addr) = listener.accept().await?;
///         println!("New connection from: {}", addr);
///
///         // 2. Spawn a new task for each connection
///         // This allows the loop to immediately return to `accept()`
///         yot_run::spawn(async move {
///             let mut buf = [0; 1024];
///
///             // 3. Handle the connection logic concurrently
///             loop {
///                 let n = match socket.read(&mut buf).await {
///                     Ok(n) if n == 0 => return, // Connection closed
///                     Ok(n) => n,
///                     Err(e) => {
///                         eprintln!("Failed to read from socket; err = {:?}", e);
///                         return;
///                     }
///                 };
///
///                 if let Err(e) = socket.write_all(&buf[0..n]).await {
///                     eprintln!("Failed to write to socket; err = {:?}", e);
///                     return;
///                 }
///             }
///         });
///     }
/// }
/// ```
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
