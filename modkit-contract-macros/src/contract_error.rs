use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Lit, MetaNameValue};

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // Extract #[contract_error(domain = "...")] from the enum
    let domain = extract_domain(&input);

    let Data::Enum(data_enum) = &input.data else {
        return syn::Error::new_spanned(&input, "ContractError can only be derived for enums")
            .to_compile_error()
            .into();
    };

    let mut error_code_arms = Vec::new();
    let mut status_code_arms = Vec::new();
    let mut problem_type_arms = Vec::new();
    let mut title_arms = Vec::new();
    let mut to_context_arms = Vec::new();
    let mut from_pd_arms = Vec::new();
    let mut display_arms = Vec::new();

    for variant in &data_enum.variants {
        let var_ident = &variant.ident;
        let error_code = var_ident.to_string().to_case(Case::UpperSnake);

        // Extract #[error(status = N, problem_type = "...")] from each variant
        let (status, problem_type_suffix) = extract_variant_attrs(variant);
        let problem_type_url =
            format!("https://errors.cyberfabric.io/{problem_type_suffix}");
        let title_str = var_ident.to_string().to_case(Case::Title);

        match &variant.fields {
            Fields::Named(fields) => {
                let field_names: Vec<_> =
                    fields.named.iter().map(|f| &f.ident).collect();
                let field_names2 = field_names.clone();
                let field_names3 = field_names.clone();

                // error_code arm
                error_code_arms.push(quote! {
                    #name::#var_ident { .. } => #error_code
                });

                // status_code arm
                status_code_arms.push(quote! {
                    #name::#var_ident { .. } => #status
                });

                // problem_type arm
                problem_type_arms.push(quote! {
                    #name::#var_ident { .. } => #problem_type_url
                });

                // title arm
                title_arms.push(quote! {
                    #name::#var_ident { .. } => #title_str
                });

                // to_context arm — serialize fields to JSON
                to_context_arms.push(quote! {
                    #name::#var_ident { #(#field_names),* } => {
                        ::serde_json::json!({ #( stringify!(#field_names2): #field_names3 ),* })
                    }
                });

                // from_problem_details arm — deserialize fields from context
                let field_extractions: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| {
                        let fname = &f.ident;
                        let fname_str = fname.as_ref().unwrap().to_string();
                        let ftype = &f.ty;
                        quote! {
                            let #fname: #ftype = ::serde_json::from_value(
                                pd.context.get(#fname_str).cloned()
                                    .unwrap_or(::serde_json::Value::Null)
                            ).unwrap_or_default();
                        }
                    })
                    .collect();

                let field_names4: Vec<_> =
                    fields.named.iter().map(|f| &f.ident).collect();

                from_pd_arms.push(quote! {
                    #error_code => {
                        #(#field_extractions)*
                        #name::#var_ident { #(#field_names4),* }
                    }
                });

                // Display arm
                display_arms.push(quote! {
                    #name::#var_ident { .. } => write!(f, "{}", #title_str)
                });
            }
            Fields::Unit => {
                error_code_arms.push(quote! {
                    #name::#var_ident => #error_code
                });
                status_code_arms.push(quote! {
                    #name::#var_ident => #status
                });
                problem_type_arms.push(quote! {
                    #name::#var_ident => #problem_type_url
                });
                title_arms.push(quote! {
                    #name::#var_ident => #title_str
                });
                to_context_arms.push(quote! {
                    #name::#var_ident => ::serde_json::Value::Null
                });
                from_pd_arms.push(quote! {
                    #error_code => #name::#var_ident
                });
                display_arms.push(quote! {
                    #name::#var_ident => write!(f, "{}", #title_str)
                });
            }
            Fields::Unnamed(_) => {
                return syn::Error::new_spanned(
                    variant,
                    "ContractError does not support tuple variants",
                )
                .to_compile_error()
                .into();
            }
        }
    }

    let expanded = quote! {
        impl #name {
            pub fn error_code(&self) -> &'static str {
                match self {
                    #(#error_code_arms,)*
                }
            }

            pub fn status_code(&self) -> u16 {
                match self {
                    #(#status_code_arms,)*
                }
            }

            pub fn problem_type(&self) -> &'static str {
                match self {
                    #(#problem_type_arms,)*
                }
            }

            pub fn title(&self) -> &'static str {
                match self {
                    #(#title_arms,)*
                }
            }

            pub fn to_problem_details(&self) -> modkit_contract_runtime::ProblemDetails {
                let context = match self {
                    #(#to_context_arms,)*
                };

                modkit_contract_runtime::ProblemDetails {
                    problem_type: self.problem_type().to_owned(),
                    title: self.title().to_owned(),
                    status: self.status_code(),
                    detail: self.to_string(),
                    error_code: self.error_code().to_owned(),
                    error_domain: #domain.to_owned(),
                    context,
                    trace_id: None,
                }
            }

            pub fn from_problem_details(pd: &modkit_contract_runtime::ProblemDetails) -> Self {
                if pd.error_domain != #domain {
                    // Unknown domain — create a fallback.
                    // The last variant is assumed to be the catch-all Internal variant.
                    return Self::__contract_error_fallback(
                        &format!("Unknown error domain '{}': {}", pd.error_domain, pd.detail)
                    );
                }

                match pd.error_code.as_str() {
                    #(#from_pd_arms,)*
                    _ => Self::__contract_error_fallback(
                        &format!("Unknown error code '{}': {}", pd.error_code, pd.detail)
                    ),
                }
            }
        }

        impl ::std::fmt::Display for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    #(#display_arms,)*
                }
            }
        }

        impl ::std::error::Error for #name {}
    };

    expanded.into()
}

fn extract_domain(input: &DeriveInput) -> String {
    for attr in &input.attrs {
        if attr.path().is_ident("contract_error") {
            if let Ok(meta_list) = attr.meta.require_list() {
                let tokens = meta_list.tokens.clone();
                if let Ok(nv) = syn::parse2::<MetaNameValue>(tokens) {
                    if nv.path.is_ident("domain") {
                        if let syn::Expr::Lit(lit) = &nv.value {
                            if let Lit::Str(s) = &lit.lit {
                                return s.value();
                            }
                        }
                    }
                }
            }
        }
    }
    panic!("ContractError requires #[contract_error(domain = \"...\")]");
}

fn extract_variant_attrs(variant: &syn::Variant) -> (u16, String) {
    let mut status = 500u16;
    let mut problem_type = "internal".to_string();

    for attr in &variant.attrs {
        if attr.path().is_ident("error") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("status") {
                    let value = meta.value()?;
                    let lit: Lit = value.parse()?;
                    if let Lit::Int(lit_int) = lit {
                        status = lit_int.base10_parse()?;
                    }
                } else if meta.path.is_ident("problem_type") {
                    let value = meta.value()?;
                    let lit: Lit = value.parse()?;
                    if let Lit::Str(lit_str) = lit {
                        problem_type = lit_str.value();
                    }
                }
                Ok(())
            });
        }
    }

    (status, problem_type)
}
