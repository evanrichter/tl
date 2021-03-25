use crate::bytes::Bytes;
use crate::stream::Stream;
use crate::util;
use std::{borrow::Cow, collections::HashMap, rc::Rc};

const OPENING_TAG: u8 = b'<';
const END_OF_TAG: &[u8] = b"</";
const SELF_CLOSING: &[u8] = b"/>";
const COMMENT: &[u8] = b"--";
const ID_ATTR: &[u8] = b"id";
const CLASS_ATTR: &[u8] = b"class";
const VOID_TAGS: &[&[u8]] = &[
    b"area", b"base", b"br", b"col", b"embed", b"hr", b"img", b"input", b"keygen", b"link",
    b"meta", b"param", b"source", b"track", b"wbr",
];

/// Stores all attributes of an HTML tag, as well as additional metadata such as `id` and `class`
#[derive(Debug, Clone)]
pub struct Attributes<'a> {
    /// Raw attributes (maps attribute key to attribute value)
    pub raw: HashMap<Bytes<'a>, Option<Bytes<'a>>>,
    /// The ID of this HTML element, if present
    pub id: Option<Bytes<'a>>,
    /// A list of class names of this HTML element, if present
    pub class: Option<Bytes<'a>>,
}

impl<'a> Attributes<'a> {
    /// Creates a new `Attributes
    pub(crate) fn new() -> Self {
        Self {
            raw: HashMap::new(),
            id: None,
            class: None,
        }
    }
}

/// Represents a single HTML element
#[derive(Debug, Clone)]
pub struct HTMLTag<'a> {
    _name: Option<Bytes<'a>>,
    _attributes: Attributes<'a>,
    _children: Vec<Rc<Node<'a>>>,
    _raw: Bytes<'a>,
}

impl<'a> HTMLTag<'a> {
    /// Creates a new HTMLTag
    pub(crate) fn new(
        name: Option<Bytes<'a>>,
        attr: Attributes<'a>,
        children: Vec<Rc<Node<'a>>>,
        raw: Bytes<'a>,
    ) -> Self {
        Self {
            _name: name,
            _attributes: attr,
            _children: children,
            _raw: raw,
        }
    }

    /// Returns the contained markup
    /// Equivalent to [Element#innerHTML](https://developer.mozilla.org/en-US/docs/Web/API/Element/innerHTML) in browsers)
    pub fn inner_html(&self) -> &Bytes<'a> {
        &self._raw
    }

    /// Returns the contained text of this element, excluding any markup
    /// Equivalent to [Element#innerText](https://developer.mozilla.org/en-US/docs/Web/API/Element/innerText) in browsers)
    /// This function may not allocate memory for a new string as it can just return the part of the tag that doesn't have markup
    /// For tags that *do* have more than one subnode, this will allocate memory
    pub fn inner_text(&self) -> Cow<'a, str> {
        let len = self._children.len();

        if len == 0 {
            // If there are no subnodes, we can just return a static, empty, string slice
            return Cow::Borrowed("");
        }

        let first = &self._children[0];

        if len == 1 {
            match &**first {
                Node::Tag(t) => return t.inner_text(),
                Node::Raw(e) => return e.as_utf8_str(),
                Node::Comment(_) => return Cow::Borrowed(""),
            }
        }

        // If there are >1 nodes, we need to allocate a new string and push each inner_text in it
        // TODO: check if String::with_capacity() is worth it
        let mut s = String::from(first.inner_text());

        for node in self._children.iter().skip(1) {
            match &**node {
                Node::Tag(t) => s.push_str(&t.inner_text()),
                Node::Raw(e) => s.push_str(&e.as_utf8_str()),
                Node::Comment(_) => { /* no op */ }
            }
        }

        Cow::Owned(s)
    }
}

/// An HTML Node
#[derive(Debug, Clone)]
pub enum Node<'a> {
    /// A regular HTML element/tag
    Tag(HTMLTag<'a>),
    /// Raw text (no particular HTML element)
    Raw(Bytes<'a>),
    /// Comment (<!-- -->)
    Comment(Bytes<'a>),
}

impl<'a> Node<'a> {
    /// Returns the inner text of this node
    pub fn inner_text(&self) -> Cow<'a, str> {
        match self {
            Node::Comment(_) => Cow::Borrowed(""),
            Node::Raw(r) => r.as_utf8_str(),
            Node::Tag(t) => t.inner_text(),
        }
    }
}

/// A list of shared HTML nodes
pub type Tree<'a> = Vec<Rc<Node<'a>>>;

/// HTML Version (<!DOCTYPE>)
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum HTMLVersion {
    /// HTML Version 5
    HTML5,
    /// Strict HTML 4.01
    StrictHTML401,
    /// Transitional HTML 4.01
    TransitionalHTML401,
    /// Frameset HTML 4.01:
    FramesetHTML401,
}

#[derive(Debug)]
pub struct Parser<'a> {
    pub stream: Stream<'a, u8>,
    pub ast: Tree<'a>,
    pub ids: HashMap<Bytes<'a>, Rc<Node<'a>>>,
    pub classes: HashMap<Bytes<'a>, Vec<Rc<Node<'a>>>>,
    pub version: Option<HTMLVersion>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &str) -> Parser {
        Parser {
            stream: Stream::new(input.as_bytes()),
            ast: Vec::new(),
            ids: HashMap::new(),
            classes: HashMap::new(),
            version: None,
        }
    }

    fn skip_whitespaces(&mut self) {
        self.read_while(&[b' ', b'\n']);
    }

    fn read_to(&mut self, terminator: &[u8]) -> &'a [u8] {
        let start = self.stream.idx;

        while !self.stream.is_eof() {
            let ch = self.stream.current_unchecked();

            let end = self.stream.idx;

            if terminator.contains(ch) {
                return self.stream.slice_unchecked(start, end);
            }

            self.stream.idx += 1;
        }

        self.stream.slice_unchecked(start, self.stream.idx)
    }

    fn read_while(&mut self, terminator: &[u8]) {
        while !self.stream.is_eof() {
            let ch = self.stream.current_unchecked();

            if !terminator.contains(ch) {
                break;
            }

            self.stream.idx += 1;
        }
    }

    fn read_ident(&mut self) -> Option<&'a [u8]> {
        let start = self.stream.idx;

        while !self.stream.is_eof() {
            let ch = self.stream.current_cpy()?;

            if !util::is_ident(ch) {
                let idx = self.stream.idx;
                return Some(self.stream.slice_unchecked(start, idx));
            }

            self.stream.idx += 1;
        }

        None
    }

    fn skip_comment(&mut self) -> Option<&'a [u8]> {
        let start = self.stream.idx;

        while !self.stream.is_eof() {
            let idx = self.stream.idx;

            if self.stream.slice_len(idx, COMMENT.len()).eq(COMMENT) {
                self.stream.idx += COMMENT.len();

                let is_end_of_comment = self.stream.expect_and_skip_cond(b'>');

                if is_end_of_comment {
                    return Some(self.stream.slice_unchecked(start, self.stream.idx));
                }
            }

            self.stream.idx += 1;
        }

        None
    }

    fn parse_attribute(&mut self) -> Option<(&'a [u8], Option<&'a [u8]>)> {
        let name = self.read_ident()?;
        self.skip_whitespaces();

        let has_value = self.stream.expect_and_skip_cond(b'=');
        if !has_value {
            return Some((name, None));
        }

        self.skip_whitespaces();
        let quote = self.stream.expect_oneof_and_skip(&[b'"', b'\''])?;

        let value = self.read_to(&[quote]);

        Some((name, Some(value)))
    }

    fn parse_attributes(&mut self) -> Attributes<'a> {
        let mut attributes = Attributes::new();

        while !self.stream.is_eof() {
            self.skip_whitespaces();

            let cur = self.stream.current_unchecked();

            if SELF_CLOSING.contains(cur) {
                break;
            }

            if let Some((k, v)) = self.parse_attribute() {
                // `id` and `class` attributes need to be handled manually,
                // as we're going to store them in a HashMap so `get_element_by_id` is O(1)

                let v: Option<Bytes<'a>> = v.map(Into::into);

                if k.eq(ID_ATTR) {
                    attributes.id = v.clone();
                } else if k.eq(CLASS_ATTR) {
                    attributes.class = v.clone();
                }

                attributes.raw.insert(k.into(), v);
            }

            self.stream.idx += 1;
        }

        attributes
    }

    fn parse_tag(&mut self, skip_current: bool) -> Option<Node<'a>> {
        let start = self.stream.idx;

        if skip_current {
            self.stream.next()?;
        }

        let markup_declaration = self.stream.expect_and_skip_cond(b'!');

        if markup_declaration {
            let is_comment = self
                .stream
                .slice(self.stream.idx, self.stream.idx + COMMENT.len())
                .eq(COMMENT);

            if is_comment {
                self.stream.idx += COMMENT.len();
                let comment = self.skip_comment()?;

                // Comments are ignored, so we return no element
                return Some(Node::Comment(comment.into()));
            }

            let name = self.read_ident()?.to_ascii_uppercase();

            if name.eq(b"DOCTYPE") {
                self.skip_whitespaces();

                let is_html5 = self
                    .read_ident()
                    .map(|ident| ident.to_ascii_uppercase().eq(b"HTML"))
                    .unwrap_or(false);

                if is_html5 {
                    self.version = Some(HTMLVersion::HTML5);
                    self.skip_whitespaces();
                    self.stream.expect_and_skip(b'>')?;
                }

                // TODO: handle DOCTYPE for HTML version <5?

                return None;
            }

            // TODO: handle the case where <! is neither DOCTYPE nor a comment
            return None;
        }

        let name = self.read_ident()?;

        let attributes = self.parse_attributes();

        let mut children = Vec::new();

        let is_self_closing = self.stream.expect_and_skip_cond(b'/');

        self.skip_whitespaces();

        if is_self_closing {
            self.stream.expect_and_skip(b'>')?;

            let raw = self.stream.slice_from(start);

            // If this is a self-closing tag (e.g. <img />), we want to return early instead of
            // reading children as the next nodes don't belong to this tag
            return Some(Node::Tag(HTMLTag::new(
                Some(name.into()),
                attributes,
                children,
                raw.into(),
            )));
        }

        self.stream.expect_and_skip(b'>')?;

        if VOID_TAGS.contains(&name) {
            let raw = self.stream.slice_from(start);

            // Some HTML tags don't have contents (e.g. <br>),
            // so we need to return early
            // Without it, any following tags would be sub-nodes
            return Some(Node::Tag(HTMLTag::new(
                Some(name.into()),
                attributes,
                children,
                raw.into(),
            )));
        }

        while !self.stream.is_eof() {
            self.skip_whitespaces();

            let idx = self.stream.idx;

            let slice = self.stream.slice(idx, idx + END_OF_TAG.len());
            if slice.eq(END_OF_TAG) {
                self.stream.idx += END_OF_TAG.len();
                let ident = self.read_ident()?;

                if !ident.eq(name) {
                    return None;
                }

                // TODO: do we want to accept the tag if it has no closing tag?
                self.stream.expect_and_skip(b'>');
                break;
            }

            // TODO: "partial" JS parser is needed to deal with script tags
            let node = self.parse_single()?;

            children.push(node);
        }

        let raw = self.stream.slice_from(start);

        Some(Node::Tag(HTMLTag::new(
            Some(name.into()),
            attributes,
            children,
            raw.into(),
        )))
    }

    fn parse_single(&mut self) -> Option<Rc<Node<'a>>> {
        self.skip_whitespaces();

        let ch = self.stream.current_cpy()?;

        if ch == OPENING_TAG {
            if let Some(tag) = self.parse_tag(true) {
                let tag_rc = Rc::new(tag);

                if let Node::Tag(tag) = &*tag_rc {
                    let (id, class) = (&tag._attributes.id, &tag._attributes.class);

                    if let Some(id) = id {
                        self.ids.insert(id.clone(), tag_rc.clone());
                    }

                    if let Some(class) = class {
                        self.process_class(class, tag_rc.clone());
                    }
                }

                Some(tag_rc)
            } else {
                None
            }
        } else {
            Some(Rc::new(Node::Raw(self.read_to(&[b'<']).into())))
        }
    }

    fn process_class(&mut self, class: &Bytes<'a>, element: Rc<Node<'a>>) {
        let raw = class.raw();

        let mut stream = Stream::new(raw);

        let mut last = 0;

        while !stream.is_eof() {
            let cur = stream.current_unchecked();

            let is_last_char = stream.idx == raw.len() - 1;

            if util::is_strict_whitespace(*cur) || is_last_char {
                let idx = if is_last_char {
                    stream.idx + 1
                } else {
                    stream.idx
                };

                let slice = stream.slice(last, idx);
                if slice.len() > 0 {
                    self.classes
                        .entry(slice.into())
                        .or_insert_with(|| Vec::new())
                        .push(element.clone());
                }

                last = idx + 1;
            }

            stream.idx += 1;
        }
    }

    pub(crate) fn parse(mut self) -> Parser<'a> {
        while !self.stream.is_eof() {
            if let Some(node) = self.parse_single() {
                self.ast.push(node);
            }
        }
        self
    }
}
