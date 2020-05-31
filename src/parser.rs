use proc_macro2::TokenTree;
use std::iter;
use syn::{
    ext::IdentExt,
    parse::{ParseStream, Parser as _},
    token, Expr, ExprBlock, ExprLit, Ident, Result, Token,
};

use crate::node::*;

struct Tag {
    ident: Ident,
    attributes: Vec<Node>,
    selfclosing: bool,
}

/// Configures the `Parser` behavior
pub struct ParserConfig {
    /// Whether the returned node tree should be nested or flat
    pub flatten: bool,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self { flatten: false }
    }
}

/// RSX Parser
pub struct Parser {
    config: ParserConfig,
}

impl Parser {
    /// Create a new parser with the given config
    pub fn new(config: ParserConfig) -> Parser {
        Parser { config }
    }

    /// Parse a given `syn::ParseStream`
    pub fn parse(&self, input: ParseStream) -> Result<Vec<Node>> {
        let mut nodes = vec![];
        while !input.cursor().eof() {
            nodes.append(&mut self.node(input)?)
        }

        Ok(nodes)
    }

    fn node(&self, input: ParseStream) -> Result<Vec<Node>> {
        let mut node = if self.text(&input.fork()).is_ok() {
            self.text(input)
        } else if self.block(&input.fork()).is_ok() {
            self.block(input)
        } else {
            self.element(input)
        }?;

        let nodes = if self.config.flatten {
            // TODO there has to be a more elegant way to do this
            let mut childs = vec![];
            childs.append(&mut node.child_nodes);
            let mut nodes = vec![node];
            nodes.append(&mut childs);
            nodes
        } else {
            vec![node]
        };

        Ok(nodes)
    }

    fn element(&self, input: ParseStream) -> Result<Node> {
        if let Ok(tag_close_ident) = self.tag_close(&input.fork()) {
            return Err(syn::Error::new(
                tag_close_ident.span(),
                "close tag has no corresponding open tag",
            ));
        }

        let tag_open = self.tag_open(input)?;

        let mut child_nodes = vec![];
        if !tag_open.selfclosing {
            loop {
                if !self.has_child_nodes(&tag_open, &input)? {
                    break;
                }

                child_nodes.append(&mut self.node(input)?);
            }

            self.tag_close(input)?;
        }

        Ok(Node {
            node_name: tag_open.ident.to_string(),
            node_value: None,
            node_type: NodeType::Element,
            attributes: tag_open.attributes,
            child_nodes,
        })
    }

    fn has_child_nodes(&self, tag_open: &Tag, input: &ParseStream) -> Result<bool> {
        // an empty input at this point means the tag wasn't closed
        if input.is_empty() {
            return Err(syn::Error::new(
                tag_open.ident.span(),
                "open tag has no corresponding close tag",
            ));
        }

        if let Ok(tag_close_ident) = self.tag_close(&input.fork()) {
            if tag_open.ident == tag_close_ident {
                // if the next token is a matching close tag then there are no child nodes
                return Ok(false);
            } else {
                // if the next token is a closing tag with a different name it's an invalid tree
                return Err(syn::Error::new(
                    tag_close_ident.span(),
                    "close tag has no corresponding open tag",
                ));
            }
        }

        Ok(true)
    }

    fn tag_open(&self, input: ParseStream) -> Result<Tag> {
        input.parse::<Token![<]>()?;
        let ident = input.parse()?;

        let mut attributes: Vec<TokenTree> = vec![];
        let selfclosing = loop {
            if let Ok(selfclosing) = self.tag_open_end(input) {
                break selfclosing;
            }

            attributes.push(input.parse()?);
        };

        let parser = move |input: ParseStream| self.attributes(input);
        let attributes = parser.parse2(attributes.into_iter().collect())?;

        Ok(Tag {
            ident,
            attributes,
            selfclosing,
        })
    }

    fn tag_open_end(&self, input: ParseStream) -> Result<bool> {
        let selfclosing = input.parse::<Option<Token![/]>>()?.is_some();
        input.parse::<Token![>]>()?;

        Ok(selfclosing)
    }

    fn tag_close(&self, input: ParseStream) -> Result<Ident> {
        input.parse::<Token![<]>()?;
        input.parse::<Token![/]>()?;
        let ident = input.parse()?;
        input.parse::<Token![>]>()?;

        Ok(ident)
    }

    fn attributes(&self, input: ParseStream) -> Result<Vec<Node>> {
        let mut nodes = vec![];
        if input.is_empty() {
            return Ok(nodes);
        }

        while self.attribute(&input.fork()).is_ok() {
            let (key, value) = self.attribute(input)?;

            nodes.push(Node {
                node_name: key,
                node_type: NodeType::Attribute,
                node_value: value,
                attributes: vec![],
                child_nodes: vec![],
            });

            if input.is_empty() {
                break;
            }
        }

        Ok(nodes)
    }

    fn attribute(&self, input: ParseStream) -> Result<(String, Option<Expr>)> {
        let key = input.call(Ident::parse_any)?.to_string();
        let eq = input.parse::<Option<Token![=]>>()?;
        let value = if eq.is_some() {
            if input.peek(token::Brace) {
                Some(self.block_expr(input)?)
            } else {
                Some(input.parse()?)
            }
        } else {
            None
        };

        Ok((key, value))
    }

    fn text(&self, input: ParseStream) -> Result<Node> {
        let text = input.parse::<ExprLit>()?.into();

        Ok(Node {
            node_name: "#text".to_owned(),
            node_value: Some(text),
            node_type: NodeType::Text,
            attributes: vec![],
            child_nodes: vec![],
        })
    }

    fn block(&self, input: ParseStream) -> Result<Node> {
        let block = self.block_expr(input)?;

        Ok(Node {
            node_name: "#block".to_owned(),
            node_value: Some(block),
            node_type: NodeType::Block,
            attributes: vec![],
            child_nodes: vec![],
        })
    }

    fn block_expr(&self, input: ParseStream) -> Result<Expr> {
        let parser = move |input: ParseStream| input.parse();
        let group: TokenTree = input.parse()?;
        let block: ExprBlock = parser.parse2(iter::once(group).collect())?;

        Ok(block.into())
    }
}
