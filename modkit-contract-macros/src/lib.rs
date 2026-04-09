use proc_macro::TokenStream;

mod contract_error;
mod rest_client;

/// Derive macro for contract error types.
///
/// # Usage
/// ```ignore
/// #[derive(ContractError)]
/// #[contract_error(domain = "poc.greeter")]
/// pub enum GreeterError {
///     #[error(status = 400, problem_type = "invalid-argument")]
///     NameTooLong { max_length: usize },
/// }
/// ```
#[proc_macro_derive(ContractError, attributes(contract_error, error))]
pub fn derive_contract_error(input: TokenStream) -> TokenStream {
    contract_error::derive(input)
}

/// Attribute macro for REST contract traits.
///
/// Applied on a trait that extends a base trait — generates REST client
/// and OpenAPI spec. Methods use `#[post(...)]`, `#[get(...)]`, `#[delete(...)]`
/// for endpoint paths, `#[streaming]` for SSE, and `#[retryable]` for retry.
///
/// ```ignore
/// #[modkit_rest_contract]
/// #[async_trait]
/// pub trait FooRest: Foo {
///     #[retryable]
///     #[post("/v1/bar")]
///     async fn bar(&self, req: &BarRequest) -> Result<BarResponse, FooError>;
/// }
/// ```
#[proc_macro_attribute]
pub fn modkit_rest_contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    rest_client::generate(attr, item)
}

/// Method-level attribute: marks a method as streaming (SSE over REST).
/// The `#[modkit_rest_contract]` macro on the trait reads and strips this.
#[proc_macro_attribute]
pub fn streaming(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Method-level attribute: marks a method as retryable with exponential backoff.
/// The `#[modkit_rest_contract]` macro on the trait reads and strips this.
#[proc_macro_attribute]
pub fn retryable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Method-level attribute: declares a POST endpoint path.
/// The `#[modkit_rest_contract]` macro on the trait reads and strips this.
#[proc_macro_attribute]
pub fn post(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Method-level attribute: declares a GET endpoint path.
/// The `#[modkit_rest_contract]` macro on the trait reads and strips this.
#[proc_macro_attribute]
pub fn get(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Method-level attribute: declares a DELETE endpoint path.
/// The `#[modkit_rest_contract]` macro on the trait reads and strips this.
#[proc_macro_attribute]
pub fn delete(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
