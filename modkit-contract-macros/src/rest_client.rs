use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Attribute, FnArg, ItemTrait, Lit, PatType, ReturnType,
    TraitItem, TraitItemFn,
};

pub fn generate(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut trait_def = parse_macro_input!(item as ItemTrait);
    let trait_name = &trait_def.ident;
    let client_name = format_ident!("{}Client", trait_name);
    let openapi_fn_name = format_ident!(
        "{}_openapi_spec",
        trait_name.to_string().to_case(Case::Snake)
    );

    // Extract the base trait name from supertrait bounds.
    // e.g., `trait FooRest: Foo` -> base is `Foo`
    let base_trait = trait_def.supertraits.iter().find_map(|bound| {
        if let syn::TypeParamBound::Trait(trait_bound) = bound {
            let seg = trait_bound.path.segments.last()?;
            let name = seg.ident.to_string();
            // Skip well-known marker traits
            if name == "Send" || name == "Sync" {
                return None;
            }
            Some(seg.ident.clone())
        } else {
            None
        }
    });

    // Extract the error type from the first method's Result<_, E>
    let err_type = trait_def.items.iter().find_map(|item| {
        if let TraitItem::Fn(method) = item {
            let (_, err) = extract_result_types(&method.sig.output);
            let err_str = err.to_string();
            if err_str != "()" { Some(err) } else { None }
        } else {
            None
        }
    }).unwrap_or_else(|| quote! { () });

    let mut method_impls = Vec::new();
    let mut openapi_paths = Vec::new();
    let mut openapi_schema_types = Vec::new();

    for item in &trait_def.items {
        let TraitItem::Fn(method) = item else {
            continue;
        };

        let is_streaming = is_streaming_method(&method.attrs);
        let is_retryable = is_retryable_method(&method.attrs);
        let has_default = method.default.is_some();

        let method_name_str = method.sig.ident.to_string();
        let (req_type, resp_type) = extract_method_types(method);

        if let Some(req_t) = &req_type {
            openapi_schema_types.push(req_t.clone());
        }
        if let Some(resp_t) = &resp_type {
            if !is_streaming {
                openapi_schema_types.push(resp_t.clone());
            }
        }

        // Extract endpoint from #[post("/v1/...")], #[get("/v1/...")], #[delete("/v1/...")]
        let endpoint = extract_endpoint_path(&method.attrs).unwrap_or_else(|| {
            // Fallback: auto-generate from method name (backwards compat)
            if is_streaming {
                let base = method_name_str
                    .replace("_stream", "")
                    .replace("stream_", "")
                    .to_case(Case::Snake);
                format!("/v1/{}/stream", base)
            } else {
                format!("/v1/{}", method_name_str.to_case(Case::Snake))
            }
        });

        // Extract HTTP method from attribute (post/get/delete), default to "post"
        let http_method = extract_http_method(&method.attrs).unwrap_or_else(|| "post".to_string());

        let req_type_name = req_type
            .as_ref()
            .map(|t| quote!(#t).to_string().replace(' ', ""))
            .unwrap_or_default();
        let resp_type_name = resp_type
            .as_ref()
            .map(|t| quote!(#t).to_string().replace(' ', ""))
            .unwrap_or_default();

        openapi_paths.push(quote! {
            (#endpoint, #method_name_str, #is_streaming, #has_default, #req_type_name, #resp_type_name, #http_method)
        });

        if !has_default {
            if is_streaming {
                method_impls.push(gen_streaming_method(method, &endpoint, &err_type));
            } else {
                method_impls.push(gen_request_response_method(method, &endpoint, &err_type, is_retryable));
            }
        }
    }

    // Strip contract-related attributes from methods before emitting the trait.
    // If there's a base trait, convert redeclared methods into default methods
    // that delegate to self (the base trait impl has the actual logic).
    for item in &mut trait_def.items {
        if let TraitItem::Fn(method) = item {
            method.attrs.retain(|a| !is_contract_method_attr(a));

            // If base trait exists and method has no default impl,
            // add a default body that calls self.method() via the base trait.
            if let Some(ref base) = base_trait {
                if method.default.is_none() {
                    let method_ident = &method.sig.ident;
                    let params: Vec<_> = method.sig.inputs.iter().filter_map(|arg| {
                        if let FnArg::Typed(PatType { pat, .. }) = arg {
                            Some(pat.clone())
                        } else {
                            None
                        }
                    }).collect();

                    let body: syn::Block = syn::parse_quote! {
                        {
                            #base::#method_ident(self, #(#params),*).await
                        }
                    };
                    method.default = Some(body);
                }
            }
        }
    }

    // Build the impl block(s) for the generated client.
    // If there's a base trait, we impl both base and rest traits.
    let impl_blocks = if let Some(ref base) = base_trait {
        quote! {
            #[cfg(feature = "rest-client")]
            #[::async_trait::async_trait]
            impl #base for #client_name {
                #(#method_impls)*
            }

            #[cfg(feature = "rest-client")]
            #[::async_trait::async_trait]
            impl #trait_name for #client_name {}
        }
    } else {
        quote! {
            #[cfg(feature = "rest-client")]
            #[::async_trait::async_trait]
            impl #trait_name for #client_name {
                #(#method_impls)*
            }
        }
    };

    let expanded = quote! {
        #trait_def

        #[cfg(feature = "rest-client")]
        pub struct #client_name {
            config: ::modkit_contract_runtime::ClientConfig,
            http: ::reqwest::Client,
        }

        #[cfg(feature = "rest-client")]
        impl #client_name {
            /// Create from a direct URL with default config.
            pub fn new(base_url: impl Into<String>) -> Self {
                let config = ::modkit_contract_runtime::ClientConfig::new(base_url);
                Self::from_config(config)
            }

            /// Create from a full client config (timeout, retry, etc.).
            pub fn from_config(config: ::modkit_contract_runtime::ClientConfig) -> Self {
                let http = ::reqwest::Client::builder()
                    .timeout(config.timeout)
                    .build()
                    .expect("failed to build HTTP client");
                Self { config, http }
            }

            async fn parse_error(&self, resp: ::reqwest::Response) -> #err_type {
                let content_type = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_owned();

                if content_type.contains("application/problem+json") {
                    match resp.json::<::modkit_contract_runtime::ProblemDetails>().await {
                        Ok(pd) => #err_type::from_problem_details(&pd),
                        Err(e) => #err_type::__contract_error_fallback(
                            &format!("Failed to parse problem details: {e}")
                        ),
                    }
                } else {
                    let status = resp.status().as_u16();
                    let body = resp.text().await.unwrap_or_default();
                    #err_type::__contract_error_fallback(
                        &format!("HTTP {status}: {body}")
                    )
                }
            }

            fn is_retryable_error(err: &#err_type) -> bool {
                let status = err.status_code();
                // 429 (rate limit), 502, 503, 504 are retryable
                matches!(status, 429 | 502 | 503 | 504)
            }
        }

        #impl_blocks

        /// Generate the OpenAPI 3.1 spec for this contract at runtime.
        /// Uses schemars to derive JSON schemas from the Rust types.
        pub fn #openapi_fn_name() -> ::serde_json::Value {
            let paths: &[(&str, &str, bool, bool, &str, &str, &str)] = &[
                #(#openapi_paths,)*
            ];

            let mut path_items = ::serde_json::Map::new();

            for &(endpoint, op_id, is_streaming, has_default, req_type, resp_type, http_method) in paths {
                let mut op = ::serde_json::json!({
                    "operationId": op_id,
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": format!("#/components/schemas/{}", req_type) }
                            }
                        }
                    }
                });

                if has_default {
                    op["description"] = ::serde_json::json!(
                        "Optional endpoint — has a default implementation. Server MAY omit."
                    );
                }

                if is_streaming {
                    op["x-modkit-streaming"] = ::serde_json::json!({
                        "transport": "sse"
                    });
                    op["responses"] = ::serde_json::json!({
                        "200": {
                            "description": "SSE event stream",
                            "content": {
                                "text/event-stream": {
                                    "schema": { "$ref": format!("#/components/schemas/{}", resp_type) }
                                }
                            }
                        },
                        "4XX": {
                            "content": {
                                "application/problem+json": {
                                    "schema": { "$ref": "#/components/schemas/ProblemDetails" }
                                }
                            }
                        }
                    });
                } else {
                    op["responses"] = ::serde_json::json!({
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": format!("#/components/schemas/{}", resp_type) }
                                }
                            }
                        },
                        "4XX": {
                            "content": {
                                "application/problem+json": {
                                    "schema": { "$ref": "#/components/schemas/ProblemDetails" }
                                }
                            }
                        }
                    });
                }

                path_items.insert(
                    endpoint.to_string(),
                    ::serde_json::json!({ http_method: op }),
                );
            }

            // Collect type schemas via schemars
            let mut schemas = ::serde_json::Map::new();
            #(
                {
                    let schema = ::schemars::schema_for!(#openapi_schema_types);
                    let schema_json = ::serde_json::to_value(&schema.schema).unwrap_or_default();
                    let type_name = stringify!(#openapi_schema_types);
                    schemas.insert(type_name.to_string(), schema_json);
                }
            )*

            // Add ProblemDetails schema
            schemas.insert("ProblemDetails".to_string(), ::serde_json::json!({
                "type": "object",
                "required": ["type", "title", "status", "detail", "error_code", "error_domain"],
                "properties": {
                    "type": { "type": "string", "description": "Error category URI" },
                    "title": { "type": "string" },
                    "status": { "type": "integer" },
                    "detail": { "type": "string" },
                    "error_code": { "type": "string", "description": "UPPER_SNAKE_CASE error identifier" },
                    "error_domain": { "type": "string", "description": "Module namespace (e.g., poc.greeter)" },
                    "context": { "type": "object", "nullable": true },
                    "trace_id": { "type": "string", "nullable": true }
                }
            }));

            ::serde_json::json!({
                "openapi": "3.1.0",
                "info": {
                    "title": concat!(stringify!(#trait_name), " Contract"),
                    "version": "v1"
                },
                "paths": path_items,
                "components": {
                    "schemas": schemas
                }
            })
        }
    };

    expanded.into()
}

fn gen_request_response_method(
    method: &TraitItemFn,
    endpoint: &str,
    err_type: &proc_macro2::TokenStream,
    is_retryable: bool,
) -> proc_macro2::TokenStream {
    let sig = &method.sig;

    let req_param = method
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(PatType { pat, .. }) = arg {
                Some(pat)
            } else {
                None
            }
        })
        .next();

    let (ok_type, _) = extract_result_types(&method.sig.output);

    let call_body = quote! {
        let resp = self.http
            .post(format!("{}{}", self.config.base_url, #endpoint))
            .json(#req_param)
            .send()
            .await
            .map_err(|e| #err_type::__contract_error_fallback(&e.to_string()))?;

        if !resp.status().is_success() {
            return Err(self.parse_error(resp).await);
        }

        resp.json::<#ok_type>().await
            .map_err(|e| #err_type::__contract_error_fallback(&e.to_string()))
    };

    if is_retryable {
        quote! {
            #sig {
                ::modkit_contract_runtime::with_retry(
                    &self.config.retry,
                    Self::is_retryable_error,
                    || async {
                        #call_body
                    },
                ).await
            }
        }
    } else {
        quote! {
            #sig {
                #call_body
            }
        }
    }
}

fn gen_streaming_method(
    method: &TraitItemFn,
    endpoint: &str,
    err_type: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let sig = &method.sig;

    let req_param = method
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(PatType { pat, .. }) = arg {
                Some(pat)
            } else {
                None
            }
        })
        .next();

    quote! {
        #sig {
            let resp = self.http
                .post(format!("{}{}", self.config.base_url, #endpoint))
                .header("Accept", "text/event-stream")
                .json(#req_param)
                .send()
                .await
                .map_err(|e| #err_type::__contract_error_fallback(&e.to_string()))?;

            if !resp.status().is_success() {
                return Err(self.parse_error(resp).await);
            }

            let byte_stream = resp.bytes_stream();
            let event_stream = ::modkit_contract_runtime::sse_stream(byte_stream);
            Ok(Box::pin(event_stream))
        }
    }
}

/// Extract the endpoint path from #[post("/v1/...")], #[get("/v1/...")], or #[delete("/v1/...")]
fn extract_endpoint_path(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        let ident = attr.path().get_ident()?;
        let name = ident.to_string();
        if name == "post" || name == "get" || name == "delete" {
            // Parse the attribute argument as a string literal
            if let Ok(lit) = attr.parse_args::<Lit>() {
                if let Lit::Str(s) = lit {
                    return Some(s.value());
                }
            }
        }
    }
    None
}

/// Extract the HTTP method name from #[post(...)], #[get(...)], or #[delete(...)]
fn extract_http_method(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if let Some(ident) = attr.path().get_ident() {
            let name = ident.to_string();
            if name == "post" || name == "get" || name == "delete" {
                return Some(name);
            }
        }
    }
    None
}

/// Extract the request type and response type from a trait method.
/// Request: the first non-self parameter's inner type (strips & and &mut)
/// Response: the Ok type from Result<T, E>
fn extract_method_types(
    method: &TraitItemFn,
) -> (Option<syn::Type>, Option<syn::Type>) {
    // Request type: first typed parameter
    let req_type = method.sig.inputs.iter().find_map(|arg| {
        if let FnArg::Typed(PatType { ty, .. }) = arg {
            // Strip references: &T or &mut T -> T
            match ty.as_ref() {
                syn::Type::Reference(r) => Some((*r.elem).clone()),
                other => Some(other.clone()),
            }
        } else {
            None
        }
    });

    // Response type: Ok type from Result<T, E>
    let resp_type = if let ReturnType::Type(_, ty) = &method.sig.output {
        if let syn::Type::Path(type_path) = ty.as_ref() {
            let last = type_path.path.segments.last().unwrap();
            if last.ident == "Result" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    args.args.first().and_then(|arg| {
                        if let syn::GenericArgument::Type(ty) = arg {
                            // For streaming, the Ok type is Pin<Box<dyn Stream<Item = T>>>
                            // We want to extract T from that
                            extract_inner_stream_item(ty).or(Some(ty.clone()))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    (req_type, resp_type)
}

/// Extract T from Pin<Box<dyn Stream<Item = T> + Send>>
fn extract_inner_stream_item(ty: &syn::Type) -> Option<syn::Type> {
    // This is a simplified extraction — works for the common pattern
    let syn::Type::Path(path) = ty else { return None };
    let seg = path.path.segments.last()?;
    if seg.ident != "Pin" { return None; }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else { return None };
    let syn::GenericArgument::Type(box_ty) = args.args.first()? else { return None };
    let syn::Type::Path(box_path) = box_ty else { return None };
    let box_seg = box_path.path.segments.last()?;
    if box_seg.ident != "Box" { return None; }
    let syn::PathArguments::AngleBracketed(box_args) = &box_seg.arguments else { return None };
    let syn::GenericArgument::Type(dyn_ty) = box_args.args.first()? else { return None };
    let syn::Type::TraitObject(trait_obj) = dyn_ty else { return None };
    for bound in &trait_obj.bounds {
        let syn::TypeParamBound::Trait(trait_bound) = bound else { continue };
        let seg = trait_bound.path.segments.last()?;
        if seg.ident != "Stream" { continue; }
        let syn::PathArguments::AngleBracketed(stream_args) = &seg.arguments else { continue };
        for arg in &stream_args.args {
            if let syn::GenericArgument::AssocType(assoc) = arg {
                if assoc.ident == "Item" {
                    return Some(assoc.ty.clone());
                }
            }
        }
    }
    None
}

/// Check if a method has `#[streaming]` as a standalone attribute
fn is_streaming_method(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("streaming"))
}

/// Check if a method has `#[retryable]` as a standalone attribute
fn is_retryable_method(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("retryable"))
}

/// Check if an attribute is one of the contract method attributes that should be stripped:
/// `#[streaming]`, `#[retryable]`, `#[post(...)]`, `#[get(...)]`, `#[delete(...)]`
fn is_contract_method_attr(attr: &Attribute) -> bool {
    let Some(ident) = attr.path().get_ident() else {
        return false;
    };
    let name = ident.to_string();
    matches!(name.as_str(), "streaming" | "retryable" | "post" | "get" | "delete")
}

fn extract_result_types(
    return_type: &ReturnType,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    // Default fallback
    let default = (quote! { () }, quote! { () });

    let ReturnType::Type(_, ty) = return_type else {
        return default;
    };

    // Look for Result<T, E> — simplified extraction
    let syn::Type::Path(type_path) = ty.as_ref() else {
        return default;
    };

    let last_segment = type_path.path.segments.last().unwrap();
    if last_segment.ident != "Result" {
        return default;
    }

    let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return default;
    };

    let mut types = args.args.iter().filter_map(|arg| {
        if let syn::GenericArgument::Type(ty) = arg {
            Some(quote! { #ty })
        } else {
            None
        }
    });

    let ok_type = types.next().unwrap_or(quote! { () });
    let err_type = types.next().unwrap_or(quote! { () });

    (ok_type, err_type)
}
