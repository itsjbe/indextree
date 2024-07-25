use either::Either;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Token,
};

#[derive(Clone, Debug)]
struct IndexNode {
    node: syn::Expr,
    children: Punctuated<Self, Token![,]>,
}

impl Parse for IndexNode {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let node = input.parse::<syn::Expr>()?;

        if input.parse::<Token![=>]>().is_err() {
            return Ok(IndexNode {
                node,
                children: Punctuated::new(),
            });
        }

        let children_stream;
        syn::braced!(children_stream in input);
        let children = children_stream.parse_terminated(Self::parse, Token![,])?;

        Ok(IndexNode { node, children })
    }
}

#[derive(Clone, Debug)]
struct IndexTree {
    arena: syn::Expr,
    root_node: syn::Expr,
    nodes: Punctuated<IndexNode, Token![,]>,
}

impl Parse for IndexTree {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let arena = input.parse::<syn::Expr>()?;

        input.parse::<Token![,]>()?;

        let root_node = input.parse::<syn::Expr>()?;

        let nodes = if input.parse::<Token![=>]>().is_ok() {
            let braced_nodes;
            syn::braced!(braced_nodes in input);
            braced_nodes.parse_terminated(IndexNode::parse, Token![,])?
        } else {
            Punctuated::new()
        };


        let _ = input.parse::<Token![,]>();

        Ok(IndexTree {
            arena,
            root_node,
            nodes,
        })
    }
}

#[derive(Clone, Debug)]
struct NestingLevelMarker;

#[proc_macro]
pub fn tree(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let IndexTree {
        arena,
        root_node,
        nodes,
    } = parse_macro_input!(input as IndexTree);

    let mut stack: Vec<Either<_, NestingLevelMarker>> =
        nodes.into_iter().map(Either::Left).rev().collect();

    // HACK(alexmozaidze): Due to the fact that specialization is unstable, we must resort to
    // autoref specialization trick.
    // https://github.com/dtolnay/case-studies/blob/master/autoref-specialization/README.md
    let mut action_buffer = quote! {
        let mut __arena: &mut ::indextree::Arena<_> = #arena;

        #[repr(transparent)]
        struct __Wrapping<__T>(::core::mem::ManuallyDrop<__T>);

        trait __ToNodeId<__T> {
            fn __to_node_id(&mut self, __arena: &mut ::indextree::Arena<__T>) -> ::indextree::NodeId;
        }

        trait __NodeIdToNodeId<__T> {
            fn __to_node_id(&mut self, __arena: &mut ::indextree::Arena<__T>) -> ::indextree::NodeId;
        }

        impl<__T> __NodeIdToNodeId<__T> for __Wrapping<::indextree::NodeId> {
            fn __to_node_id(&mut self, __arena: &mut ::indextree::Arena<__T>) -> ::indextree::NodeId {
                unsafe { ::core::mem::ManuallyDrop::take(&mut self.0) }
            }
        }

        impl<__T> __ToNodeId<__T> for &mut __Wrapping<__T> {
            fn __to_node_id(&mut self, __arena: &mut ::indextree::Arena<__T>) -> ::indextree::NodeId {
                ::indextree::Arena::new_node(__arena, unsafe { ::core::mem::ManuallyDrop::take(&mut self.0) })
            }
        }

        let __root_node: ::indextree::NodeId = {
            let mut __root_node = __Wrapping(::core::mem::ManuallyDrop::new(#root_node));
            (&mut __root_node).__to_node_id(__arena)
        };
        let mut __node: ::indextree::NodeId = __root_node;
        let mut __last: ::indextree::NodeId;
    };

    while let Some(item) = stack.pop() {
        let Either::Left(IndexNode { node, children }) = item else {
            action_buffer.extend(quote! {
                let __temp = ::indextree::Arena::get(__arena, __node);
                let __temp = ::core::option::Option::unwrap(__temp);
                let __temp = ::indextree::Node::parent(__temp);
                let __temp = ::core::option::Option::unwrap(__temp);
                __node = __temp;
            });
            continue;
        };

        action_buffer.extend(quote! {
            __last = __node.append_value(#node, __arena);
        });

        if children.is_empty() {
            continue;
        }

        // going one level deeper
        stack.push(Either::Right(NestingLevelMarker));
        action_buffer.extend(quote! {
            __node = __last;
        });
        stack.extend(children.into_iter().map(Either::Left).rev());
    }

    quote! {{
        #action_buffer;
        __root_node
    }}
    .into()
}