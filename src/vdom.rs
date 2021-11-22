use crate::parser::NodeHandle;
use crate::ParserOptions;
use crate::{bytes::AsBytes, parser::HTMLVersion};
use crate::{Node, Parser};
use std::marker::PhantomData;

/// VDom represents a [Document Object Model](https://developer.mozilla.org/en/docs/Web/API/Document_Object_Model)
///
/// It is the result of parsing an HTML document.
/// Internally it is only a wrapper around the [`Parser`] struct, but you do not need to know much about the [`Parser`] struct except for that all of the HTML tags are stored in here.
/// Many functions of the public API take a reference to a [`Parser`] as a parameter to resolve [`NodeHandle`]s to [`Node`]s.
#[derive(Debug)]
pub struct VDom<'a> {
    /// Internal parser
    parser: Parser<'a>,
}

impl<'a> From<Parser<'a>> for VDom<'a> {
    fn from(parser: Parser<'a>) -> Self {
        Self { parser }
    }
}

impl<'a> VDom<'a> {
    /// Returns a reference to the underlying parser
    #[inline]
    pub fn parser(&self) -> &Parser<'a> {
        &self.parser
    }

    /// Finds an element by its `id` attribute.
    pub fn get_element_by_id<'b, S: ?Sized>(&'b self, id: &'b S) -> Option<NodeHandle>
    where
        S: AsBytes,
    {
        let bytes = id.as_bytes();
        let parser = self.parser();

        if parser.options.is_tracking_ids() {
            parser.ids.get(&bytes).copied()
        } else {
            self.nodes()
                .iter()
                .enumerate()
                .find(|(_, node)| {
                    node.as_tag().map_or(false, |tag| {
                        tag._attributes.id.as_ref().map_or(false, |x| x.eq(&bytes))
                    })
                })
                .map(|(id, _)| NodeHandle::new(id))
        }
    }

    /// Returns a list of elements that match a given class name.
    pub fn get_elements_by_class_name<'b, S: ?Sized>(
        &'b self,
        id: &'b S,
    ) -> Box<dyn Iterator<Item = NodeHandle> + '_>
    where
        S: AsBytes,
    {
        let bytes = id.as_bytes();
        let parser = self.parser();

        if parser.options.is_tracking_classes() {
            parser
                .classes
                .get(&bytes)
                .map(|x| Box::new(x.iter().cloned()) as Box<dyn Iterator<Item = NodeHandle>>)
                .unwrap_or_else(|| Box::new(std::iter::empty()))
        } else {
            let member = bytes.as_utf8_str();
            let iter = self
                .nodes()
                .iter()
                .enumerate()
                .filter_map(move |(id, node)| {
                    node.as_tag().and_then(|tag| {
                        tag._attributes
                            .is_class_member(member.as_ref())
                            .then(|| NodeHandle::new(id))
                    })
                });

            Box::new(iter)
        }
    }

    /// Returns a slice of *all* the elements in the HTML document
    ///
    /// The difference between `children()` and `nodes()` is that children only returns the immediate children of the root node,
    /// while `nodes()` returns all nodes, including nested tags.
    pub fn nodes(&self) -> &[Node<'a>] {
        &self.parser.tags
    }

    /// Returns the topmost subnodes ("children") of this DOM
    pub fn children(&self) -> &[NodeHandle] {
        &self.parser.ast
    }

    /// Returns the HTML version.
    /// This is determined by the `<!DOCTYPE>` tag
    pub fn version(&self) -> Option<HTMLVersion> {
        self.parser.version
    }

    /// Calls the given closure with each tag as parameter
    ///
    /// The closure must return a boolean, indicating whether it should stop iterating
    /// Returning `true` will break the loop
    #[deprecated(
        since = "0.3.0",
        note = "please use `nodes().iter().find(...)` instead"
    )]
    pub fn find_node<F>(&self, mut f: F) -> Option<NodeHandle>
    where
        F: FnMut(&Node<'a>) -> bool,
    {
        let parser = self.parser();

        for node in self.children() {
            let node = node.get(parser).and_then(|x| x.find_node(parser, &mut f));

            if node.is_some() {
                return node;
            }
        }

        None
    }
}

/// A RAII guarded version of VDom
///
/// The input string is freed once this struct goes out of scope.
/// The only way to construct this is by calling `parse_owned()`.
#[derive(Debug)]
pub struct VDomGuard<'a> {
    /// Pointer to leaked input string
    ptr: *mut str,
    /// Wrapped VDom instance
    dom: VDom<'a>,
    /// PhantomData for self.dom
    _phantom: PhantomData<&'a str>,
}

// SAFETY: The string is leaked and pinned to a memory location
unsafe impl<'a> Send for VDomGuard<'a> {}
unsafe impl<'a> Sync for VDomGuard<'a> {}

impl<'a> VDomGuard<'a> {
    /// Parses the input string
    pub(crate) fn parse(input: String, options: ParserOptions) -> VDomGuard<'a> {
        let ptr = Box::into_raw(input.into_boxed_str());

        // SAFETY: Shortening the lifetime of the input string is fine, as it's `'static`
        let input_extended: &'a str = unsafe { &*ptr };

        let parser = Parser::new(input_extended, options).parse();

        Self {
            ptr,
            dom: VDom::from(parser),
            _phantom: PhantomData,
        }
    }
}

impl<'a> VDomGuard<'a> {
    /// Returns a reference to the inner DOM.
    ///
    /// The lifetime of `VDOM` is bound to self so that elements cannot outlive this `VDomGuard` struct.
    pub fn get_ref<'b>(&'a self) -> &'b VDom<'a> {
        &self.dom
    }
}

impl<'a> Drop for VDomGuard<'a> {
    fn drop(&mut self) {
        // SAFETY: We made this pointer in VDomGuard::parse() so we know it is properly aligned and non-null
        drop(unsafe { Box::from_raw(self.ptr) });
    }
}
