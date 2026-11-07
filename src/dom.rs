//! A small arena DOM built by html5ever.
//!
//! Nodes live in one `Vec` and refer to each other by [`NodeId`], so the
//! tree can be mutated freely (moving, renaming, removing nodes) without
//! reference counting. Removed nodes stay in the arena until the document is
//! dropped. The API mirrors the parts of the DOM that Readability uses, with
//! the same semantics (`textContent`, `getElementsByTagName`, `innerHTML`, ...).

use std::borrow::Cow;
use std::cell::{Ref, RefCell};
use std::fmt::Write as _;
use std::num::NonZeroU32;

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, LocalName, Namespace, ParseOpts, QualName, local_name, ns};

/// Index of a node in a [`Document`].
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    #[inline]
    fn new(index: usize) -> Self {
        NodeId(NonZeroU32::new(u32::try_from(index + 1).expect("too many nodes")).unwrap())
    }

    /// Position of the node in the arena (stable for the document's lifetime).
    #[inline]
    pub fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

#[derive(Debug, Clone)]
pub struct ElementData {
    pub name: QualName,
    pub attrs: Vec<Attribute>,
    pub template_contents: Option<NodeId>,
    mathml_annotation_xml_integration_point: bool,
}

#[derive(Debug, Clone)]
pub enum NodeData {
    Document,
    /// A document fragment; `owner` is set for a template's contents.
    Fragment {
        owner: Option<NodeId>,
    },
    Doctype {
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    },
    Text(StrTendril),
    Comment(StrTendril),
    ProcessingInstruction {
        target: StrTendril,
        data: StrTendril,
    },
    Element(ElementData),
}

#[derive(Debug, Clone)]
pub struct Node {
    parent: Option<NodeId>,
    prev: Option<NodeId>,
    next: Option<NodeId>,
    first: Option<NodeId>,
    last: Option<NodeId>,
    pub data: NodeData,
}

impl Node {
    fn new(data: NodeData) -> Self {
        Node { parent: None, prev: None, next: None, first: None, last: None, data }
    }
}

/// An HTML document.
pub struct Document {
    nodes: Vec<Node>,
    url: Option<url::Url>,
}

/// html5ever `ElemName` for an element borrowed out of the sink's `RefCell`.
type BorrowedName<'a> = Ref<'a, QualName>;

struct Sink {
    nodes: RefCell<Vec<Node>>,
}

impl Sink {
    fn push(&self, data: NodeData) -> NodeId {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(Node::new(data));
        NodeId::new(nodes.len() - 1)
    }

    fn append_text_or_node(&self, child: NodeOrText<NodeId>, prev: Option<NodeId>) -> Option<NodeId> {
        match child {
            NodeOrText::AppendText(text) => {
                if let Some(prev) = prev {
                    let mut nodes = self.nodes.borrow_mut();
                    if let NodeData::Text(existing) = &mut nodes[prev.index()].data {
                        existing.push_tendril(&text);
                        return None;
                    }
                }
                Some(self.push(NodeData::Text(text)))
            }
            NodeOrText::AppendNode(node) => Some(node),
        }
    }
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Vec<Node>;
    type ElemName<'a> = BorrowedName<'a>;

    fn finish(self) -> Vec<Node> {
        self.nodes.into_inner()
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> NodeId {
        NodeId::new(0)
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> BorrowedName<'a> {
        Ref::map(self.nodes.borrow(), |nodes| match &nodes[target.index()].data {
            NodeData::Element(e) => &e.name,
            _ => panic!("not an element"),
        })
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> NodeId {
        let template_contents = if flags.template { Some(self.push(NodeData::Fragment { owner: None })) } else { None };
        let id = self.push(NodeData::Element(ElementData {
            name,
            attrs,
            template_contents,
            mathml_annotation_xml_integration_point: flags.mathml_annotation_xml_integration_point,
        }));
        if let Some(t) = template_contents {
            self.nodes.borrow_mut()[t.index()].data = NodeData::Fragment { owner: Some(id) };
        }
        id
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.push(NodeData::Comment(text))
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> NodeId {
        self.push(NodeData::ProcessingInstruction { target, data })
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        let last = self.nodes.borrow()[parent.index()].last;
        if let Some(node) = self.append_text_or_node(child, last) {
            link_append(&mut self.nodes.borrow_mut(), *parent, node);
        }
    }

    fn append_based_on_parent_node(&self, element: &NodeId, prev_element: &NodeId, child: NodeOrText<NodeId>) {
        if self.nodes.borrow()[element.index()].parent.is_some() {
            self.append_before_sibling(element, child)
        } else {
            self.append(prev_element, child)
        }
    }

    fn append_doctype_to_document(&self, name: StrTendril, public_id: StrTendril, system_id: StrTendril) {
        let node = self.push(NodeData::Doctype { name, public_id, system_id });
        link_append(&mut self.nodes.borrow_mut(), NodeId::new(0), node);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        match &self.nodes.borrow()[target.index()].data {
            NodeData::Element(ElementData { template_contents: Some(t), .. }) => *t,
            _ => panic!("not a template element"),
        }
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &NodeId, child: NodeOrText<NodeId>) {
        let prev = self.nodes.borrow()[sibling.index()].prev;
        if let Some(node) = self.append_text_or_node(child, prev) {
            let mut nodes = self.nodes.borrow_mut();
            detach(&mut nodes, node);
            let parent = nodes[sibling.index()].parent.expect("sibling without parent");
            link_before(&mut nodes, parent, node, *sibling);
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<Attribute>) {
        let mut nodes = self.nodes.borrow_mut();
        if let NodeData::Element(e) = &mut nodes[target.index()].data {
            for attr in attrs {
                if !e.attrs.iter().any(|a| a.name == attr.name) {
                    e.attrs.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        detach(&mut self.nodes.borrow_mut(), *target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut nodes = self.nodes.borrow_mut();
        let mut child = nodes[node.index()].first;
        while let Some(c) = child {
            child = nodes[c.index()].next;
            detach(&mut nodes, c);
            link_append(&mut nodes, *new_parent, c);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        match &self.nodes.borrow()[handle.index()].data {
            NodeData::Element(e) => e.mathml_annotation_xml_integration_point,
            _ => false,
        }
    }
}

fn detach(nodes: &mut [Node], id: NodeId) {
    let (parent, prev, next) = {
        let n = &mut nodes[id.index()];
        (n.parent.take(), n.prev.take(), n.next.take())
    };
    let Some(parent) = parent else { return };
    match prev {
        Some(p) => nodes[p.index()].next = next,
        None => nodes[parent.index()].first = next,
    }
    match next {
        Some(n) => nodes[n.index()].prev = prev,
        None => nodes[parent.index()].last = prev,
    }
}

/// Appends a detached node as the last child of `parent`.
fn link_append(nodes: &mut [Node], parent: NodeId, child: NodeId) {
    let last = nodes[parent.index()].last;
    {
        let c = &mut nodes[child.index()];
        c.parent = Some(parent);
        c.prev = last;
        c.next = None;
    }
    match last {
        Some(l) => nodes[l.index()].next = Some(child),
        None => nodes[parent.index()].first = Some(child),
    }
    nodes[parent.index()].last = Some(child);
}

/// Inserts a detached node before `reference` (a child of `parent`).
fn link_before(nodes: &mut [Node], parent: NodeId, child: NodeId, reference: NodeId) {
    let prev = nodes[reference.index()].prev;
    {
        let c = &mut nodes[child.index()];
        c.parent = Some(parent);
        c.prev = prev;
        c.next = Some(reference);
    }
    nodes[reference.index()].prev = Some(child);
    match prev {
        Some(p) => nodes[p.index()].next = Some(child),
        None => nodes[parent.index()].first = Some(child),
    }
}

/// HTML void elements (no end tag when serialized).
fn is_void(name: &LocalName) -> bool {
    matches!(
        *name,
        local_name!("area")
            | local_name!("base")
            | local_name!("basefont")
            | local_name!("bgsound")
            | local_name!("br")
            | local_name!("col")
            | local_name!("embed")
            | local_name!("frame")
            | local_name!("hr")
            | local_name!("img")
            | local_name!("input")
            | local_name!("keygen")
            | local_name!("link")
            | local_name!("meta")
            | local_name!("param")
            | local_name!("source")
            | local_name!("track")
            | local_name!("wbr")
    )
}

/// Elements whose text children are serialized without escaping.
fn is_raw_text_parent(name: &LocalName) -> bool {
    // `noscript` is escaped normally: documents are parsed with scripting
    // disabled, as jsdom does.
    matches!(
        *name,
        local_name!("style")
            | local_name!("script")
            | local_name!("xmp")
            | local_name!("iframe")
            | local_name!("noembed")
            | local_name!("noframes")
            | local_name!("plaintext")
    )
}

/// The qualified name of an attribute (`Attr.name`).
pub fn attr_qualified_name(name: &QualName) -> Cow<'_, str> {
    match &name.prefix {
        None => Cow::Borrowed(&*name.local),
        Some(prefix) => Cow::Owned(format!("{}:{}", &**prefix, &*name.local)),
    }
}

/// Attribute name as written by the HTML serializer.
fn serialized_attr_name(name: &QualName, out: &mut String) {
    if name.ns == ns!() {
        out.push_str(&name.local);
    } else if name.ns == ns!(xml) {
        out.push_str("xml:");
        out.push_str(&name.local);
    } else if name.ns == ns!(xmlns) {
        if name.local == local_name!("xmlns") {
            out.push_str("xmlns");
        } else {
            out.push_str("xmlns:");
            out.push_str(&name.local);
        }
    } else if name.ns == ns!(xlink) {
        out.push_str("xlink:");
        out.push_str(&name.local);
    } else {
        out.push_str(&attr_qualified_name(name));
    }
}

fn escape_text(s: &str, out: &mut String) {
    let mut last = 0;
    for (i, c) in s.char_indices() {
        let rep = match c {
            '&' => "&amp;",
            '\u{a0}' => "&nbsp;",
            '<' => "&lt;",
            '>' => "&gt;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + c.len_utf8();
    }
    out.push_str(&s[last..]);
}

fn escape_attr(s: &str, out: &mut String) {
    let mut last = 0;
    for (i, c) in s.char_indices() {
        let rep = match c {
            '&' => "&amp;",
            '\u{a0}' => "&nbsp;",
            '"' => "&quot;",
            '<' => "&lt;",
            '>' => "&gt;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + c.len_utf8();
    }
    out.push_str(&s[last..]);
}

impl Document {
    /// Parses an HTML document. `url` becomes the document URL (used for
    /// `<base href>` and for resolving relative links).
    pub fn parse(html: &str, url: Option<url::Url>) -> Document {
        let sink = Sink { nodes: RefCell::new(Vec::with_capacity(html.len() / 32 + 16)) };
        sink.push(NodeData::Document);
        let mut opts = ParseOpts::default();
        // As in jsdom (scripts don't run): <noscript> content is parsed as markup.
        opts.tree_builder.scripting_enabled = false;
        let nodes = html5ever::parse_document(sink, opts).one(StrTendril::from(html));
        Document { nodes, url }
    }

    pub fn document(&self) -> NodeId {
        NodeId::new(0)
    }

    pub fn url(&self) -> Option<&url::Url> {
        self.url.as_ref()
    }

    /// `document.documentURI`.
    pub fn document_uri(&self) -> &str {
        self.url.as_ref().map_or("about:blank", |u| u.as_str())
    }

    /// The document base URL (`document.baseURI`): the first `<base href>`
    /// resolved against the document URL, or the document URL.
    pub fn base_url(&self) -> Option<url::Url> {
        let doc = self.document();
        let mut node = self.first_child(doc);
        while let Some(n) = node {
            if self.is_html(n, &local_name!("base"))
                && let Some(href) = self.attr(n, "href")
            {
                return match &self.url {
                    Some(u) => u.join(href).ok().or_else(|| Some(u.clone())),
                    None => url::Url::parse(href).ok(),
                };
            }
            node = self.next_in(n, doc);
        }
        self.url.clone()
    }

    /// Serialized `document.baseURI`.
    pub fn base_uri(&self) -> String {
        self.base_url().map_or_else(|| "about:blank".to_owned(), |u| u.as_str().to_owned())
    }

    #[inline]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    #[inline]
    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    /// Number of nodes ever allocated (including removed ones).
    pub fn arena_len(&self) -> usize {
        self.nodes.len()
    }

    // ----- creation -----

    fn push(&mut self, data: NodeData) -> NodeId {
        self.nodes.push(Node::new(data));
        NodeId::new(self.nodes.len() - 1)
    }

    /// `document.createElement(name)` (HTML namespace, lower-cased name).
    pub fn create_element(&mut self, name: &str) -> NodeId {
        let local = LocalName::from(name.to_ascii_lowercase());
        let template = local == local_name!("template");
        let template_contents = if template { Some(self.push(NodeData::Fragment { owner: None })) } else { None };
        let id = self.push(NodeData::Element(ElementData {
            name: QualName::new(None, ns!(html), local),
            attrs: Vec::new(),
            template_contents,
            mathml_annotation_xml_integration_point: false,
        }));
        if let Some(t) = template_contents {
            self.node_mut(t).data = NodeData::Fragment { owner: Some(id) };
        }
        id
    }

    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.push(NodeData::Text(StrTendril::from(text)))
    }

    pub fn create_fragment(&mut self) -> NodeId {
        self.push(NodeData::Fragment { owner: None })
    }

    /// `node.cloneNode(true)`. Template contents are cloned too.
    pub fn deep_clone(&mut self, root: NodeId) -> NodeId {
        let copy = self.shallow_clone(root);
        // (source, clone) pairs whose children still need copying. Iterative,
        // including template contents, so deep nesting cannot overflow the stack.
        let mut stack = vec![(root, copy)];
        while let Some((src, dst)) = stack.pop() {
            if let Some(t) = self.element(src).and_then(|e| e.template_contents) {
                let t2 = self.push(NodeData::Fragment { owner: Some(dst) });
                if let Some(e) = self.element_mut(dst) {
                    e.template_contents = Some(t2);
                }
                stack.push((t, t2));
            }
            let mut child = self.node(src).first;
            while let Some(c) = child {
                let cc = self.shallow_clone(c);
                link_append(&mut self.nodes, dst, cc);
                stack.push((c, cc));
                child = self.node(c).next;
            }
        }
        copy
    }

    /// Copies a node without children (or template contents, which
    /// [`Self::deep_clone`] fills in).
    fn shallow_clone(&mut self, id: NodeId) -> NodeId {
        let mut data = self.node(id).data.clone();
        match &mut data {
            NodeData::Fragment { owner } => *owner = None,
            NodeData::Element(e) => e.template_contents = None,
            _ => {}
        }
        self.push(data)
    }

    // ----- navigation -----

    #[inline]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }
    #[inline]
    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).first
    }
    #[inline]
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).last
    }
    #[inline]
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).next
    }
    #[inline]
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).prev
    }

    pub fn first_element_child(&self, id: NodeId) -> Option<NodeId> {
        let mut c = self.first_child(id);
        while let Some(n) = c {
            if self.is_element(n) {
                return Some(n);
            }
            c = self.next_sibling(n);
        }
        None
    }

    pub fn next_element_sibling(&self, id: NodeId) -> Option<NodeId> {
        let mut c = self.next_sibling(id);
        while let Some(n) = c {
            if self.is_element(n) {
                return Some(n);
            }
            c = self.next_sibling(n);
        }
        None
    }

    pub fn prev_element_sibling(&self, id: NodeId) -> Option<NodeId> {
        let mut c = self.prev_sibling(id);
        while let Some(n) = c {
            if self.is_element(n) {
                return Some(n);
            }
            c = self.prev_sibling(n);
        }
        None
    }

    /// `node.childNodes` (a snapshot).
    pub fn child_nodes(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        let mut c = self.first_child(id);
        while let Some(n) = c {
            v.push(n);
            c = self.next_sibling(n);
        }
        v
    }

    /// `node.children` (a snapshot).
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        let mut c = self.first_element_child(id);
        while let Some(n) = c {
            v.push(n);
            c = self.next_element_sibling(n);
        }
        v
    }

    /// `node.children.length`, without allocating.
    pub fn element_child_count(&self, id: NodeId) -> usize {
        let mut count = 0;
        let mut c = self.first_child(id);
        while let Some(n) = c {
            if self.is_element(n) {
                count += 1;
            }
            c = self.next_sibling(n);
        }
        count
    }

    pub fn child_count(&self, id: NodeId) -> usize {
        let mut count = 0;
        let mut c = self.first_child(id);
        while let Some(n) = c {
            count += 1;
            c = self.next_sibling(n);
        }
        count
    }

    /// The next node after `node` in tree order, staying inside `root`.
    #[inline]
    pub fn next_in(&self, node: NodeId, root: NodeId) -> Option<NodeId> {
        if let Some(c) = self.first_child(node) {
            return Some(c);
        }
        self.next_skipping_children(node, root)
    }

    /// Like [`Self::next_in`] but does not descend into `node`.
    #[inline]
    pub fn next_skipping_children(&self, mut node: NodeId, root: NodeId) -> Option<NodeId> {
        loop {
            if node == root {
                return None;
            }
            if let Some(s) = self.next_sibling(node) {
                return Some(s);
            }
            node = self.parent(node)?;
        }
    }

    /// Depth-first walk over the descendants of `root`: `f(node, true)` is
    /// called when a node is reached, `f(node, false)` after its subtree.
    #[inline]
    pub fn walk(&self, root: NodeId, mut f: impl FnMut(NodeId, bool)) {
        let Some(mut node) = self.first_child(root) else { return };
        loop {
            f(node, true);
            if let Some(c) = self.first_child(node) {
                node = c;
                continue;
            }
            loop {
                f(node, false);
                if let Some(s) = self.next_sibling(node) {
                    node = s;
                    break;
                }
                match self.parent(node) {
                    Some(p) if p != root => node = p,
                    _ => return,
                }
            }
        }
    }

    /// Iterates over the descendants of `root` in tree order (excluding `root`).
    pub fn descendants(&self, root: NodeId) -> Descendants<'_> {
        Descendants { doc: self, root, next: self.first_child(root) }
    }

    /// Is `ancestor` an inclusive ancestor of `node`?
    pub fn contains(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            match self.parent(node) {
                Some(p) => node = p,
                None => return false,
            }
        }
    }

    // ----- node kinds -----

    #[inline]
    pub fn is_element(&self, id: NodeId) -> bool {
        matches!(self.node(id).data, NodeData::Element(_))
    }

    #[inline]
    pub fn is_text(&self, id: NodeId) -> bool {
        matches!(self.node(id).data, NodeData::Text(_))
    }

    #[inline]
    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        match &self.node(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    #[inline]
    fn element_mut(&mut self, id: NodeId) -> Option<&mut ElementData> {
        match &mut self.node_mut(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    /// The local name of an HTML-namespace element. `tagName` of an HTML
    /// element is this name upper-cased; the tag name of other elements is
    /// never equal to an upper-case HTML name, so comparisons such as
    /// `node.tagName === "DIV"` become `doc.html_name(node) == Some(div)`.
    #[inline]
    pub fn html_name(&self, id: NodeId) -> Option<&LocalName> {
        match &self.node(id).data {
            NodeData::Element(e) if e.name.ns == ns!(html) => Some(&e.name.local),
            _ => None,
        }
    }

    #[inline]
    pub fn is_html(&self, id: NodeId, name: &LocalName) -> bool {
        self.html_name(id) == Some(name)
    }

    #[inline]
    pub fn local_name(&self, id: NodeId) -> Option<&LocalName> {
        self.element(id).map(|e| &e.name.local)
    }

    #[inline]
    pub fn namespace(&self, id: NodeId) -> Option<&Namespace> {
        self.element(id).map(|e| &e.name.ns)
    }

    /// `element.tagName`.
    pub fn tag_name(&self, id: NodeId) -> Option<String> {
        let e = self.element(id)?;
        Some(if e.name.ns == ns!(html) {
            e.name.local.to_ascii_uppercase().to_string()
        } else {
            match &e.name.prefix {
                Some(p) => format!("{}:{}", &**p, &*e.name.local),
                None => e.name.local.to_string(),
            }
        })
    }

    /// Text of a text, comment or processing-instruction node (`CharacterData.data`).
    pub fn char_data(&self, id: NodeId) -> Option<&str> {
        match &self.node(id).data {
            NodeData::Text(t) | NodeData::Comment(t) => Some(t),
            NodeData::ProcessingInstruction { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Text of a text node.
    #[inline]
    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.node(id).data {
            NodeData::Text(t) => Some(t),
            _ => None,
        }
    }

    // ----- attributes -----

    /// `element.getAttribute(name)` (`name` must be lower case).
    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        let e = self.element(id)?;
        e.attrs.iter().find(|a| attr_name_is(&a.name, name)).map(|a| &*a.value)
    }

    pub fn has_attr(&self, id: NodeId, name: &str) -> bool {
        self.attr(id, name).is_some()
    }

    /// `element.setAttribute(name, value)`.
    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        let Some(e) = self.element_mut(id) else { return };
        if let Some(a) = e.attrs.iter_mut().find(|a| attr_name_is(&a.name, name)) {
            a.value = StrTendril::from(value);
        } else {
            e.attrs.push(Attribute {
                name: QualName::new(None, ns!(), LocalName::from(name)),
                value: StrTendril::from(value),
            });
        }
    }

    /// `element.setAttributeNode(attr.cloneNode())`: replaces the attribute
    /// with the same namespace and local name, or appends it.
    pub fn set_attr_node(&mut self, id: NodeId, attr: Attribute) {
        let Some(e) = self.element_mut(id) else { return };
        if let Some(a) = e.attrs.iter_mut().find(|a| a.name.ns == attr.name.ns && a.name.local == attr.name.local) {
            *a = attr;
        } else {
            e.attrs.push(attr);
        }
    }

    pub fn remove_attr(&mut self, id: NodeId, name: &str) {
        if let Some(e) = self.element_mut(id)
            && let Some(pos) = e.attrs.iter().position(|a| attr_name_is(&a.name, name))
        {
            e.attrs.remove(pos);
        }
    }

    pub fn attrs(&self, id: NodeId) -> &[Attribute] {
        self.element(id).map_or(&[], |e| &e.attrs)
    }

    /// `element.id`.
    pub fn id_attr(&self, id: NodeId) -> &str {
        self.attr(id, "id").unwrap_or("")
    }

    /// `element.className` (for HTML elements; empty for other nodes).
    pub fn class_name(&self, id: NodeId) -> &str {
        self.attr(id, "class").unwrap_or("")
    }

    // ----- mutation -----

    /// `node.remove()`.
    pub fn remove(&mut self, id: NodeId) {
        detach(&mut self.nodes, id);
    }

    /// `parent.appendChild(child)`; a fragment's children are moved instead.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if matches!(self.node(child).data, NodeData::Fragment { .. }) {
            while let Some(c) = self.first_child(child) {
                detach(&mut self.nodes, c);
                link_append(&mut self.nodes, parent, c);
            }
            return;
        }
        detach(&mut self.nodes, child);
        link_append(&mut self.nodes, parent, child);
    }

    /// `parent.insertBefore(child, reference)`.
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        let Some(reference) = reference else {
            self.append_child(parent, child);
            return;
        };
        if matches!(self.node(child).data, NodeData::Fragment { .. }) {
            while let Some(c) = self.first_child(child) {
                detach(&mut self.nodes, c);
                link_before(&mut self.nodes, parent, c, reference);
            }
            return;
        }
        if child == reference {
            return;
        }
        detach(&mut self.nodes, child);
        link_before(&mut self.nodes, parent, child, reference);
    }

    /// `old.parentNode.replaceChild(new, old)`.
    pub fn replace(&mut self, old: NodeId, new: NodeId) {
        if old == new {
            return;
        }
        let Some(parent) = self.parent(old) else { return };
        detach(&mut self.nodes, new);
        let next = self.next_sibling(old);
        detach(&mut self.nodes, old);
        self.insert_before(parent, new, next);
    }

    /// Renames an element in place (keeps attributes and children).
    pub fn rename(&mut self, id: NodeId, name: &str) {
        let local = LocalName::from(name.to_ascii_lowercase());
        if let Some(e) = self.element_mut(id) {
            e.name = QualName::new(None, ns!(html), local);
        }
    }

    /// Removes all children of `id`.
    pub fn clear_children(&mut self, id: NodeId) {
        while let Some(c) = self.first_child(id) {
            detach(&mut self.nodes, c);
        }
    }

    // ----- queries -----

    /// `root.getElementsByTagName(name)` / `querySelectorAll(name)` for a
    /// single lower-case name, in tree order. `"*"` matches every element.
    pub fn elements_by_tag(&self, root: NodeId, name: &LocalName) -> Vec<NodeId> {
        let star = &**name == "*";
        self.descendants(root)
            .filter(|&n| match self.local_name(n) {
                Some(l) => star || l == name,
                None => false,
            })
            .collect()
    }

    /// `querySelectorAll("a,b,c")` for lower-case tag names, in tree order.
    pub fn elements_by_tags(&self, root: NodeId, names: &[LocalName]) -> Vec<NodeId> {
        self.descendants(root)
            .filter(|&n| match self.local_name(n) {
                Some(l) => names.contains(l),
                None => false,
            })
            .collect()
    }

    pub fn first_element_by_tag(&self, root: NodeId, name: &LocalName) -> Option<NodeId> {
        self.descendants(root).find(|&n| self.local_name(n) == Some(name))
    }

    pub fn count_elements_by_tag(&self, root: NodeId, name: &LocalName) -> usize {
        self.descendants(root).filter(|&n| self.local_name(n) == Some(name)).count()
    }

    pub fn document_element(&self) -> Option<NodeId> {
        self.first_element_child(self.document())
    }

    pub fn body(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        if !self.is_html(html, &local_name!("html")) {
            return None;
        }
        let mut c = self.first_element_child(html);
        while let Some(n) = c {
            if self.is_html(n, &local_name!("body")) || self.is_html(n, &local_name!("frameset")) {
                return Some(n);
            }
            c = self.next_element_sibling(n);
        }
        None
    }

    /// `document.title`: the first `<title>`'s text with ASCII whitespace
    /// stripped and collapsed.
    pub fn title(&self) -> String {
        let doc = self.document();
        // The HTML `<title>`, not an SVG `<title>` (an accessibility label).
        let Some(t) = self.descendants(doc).find(|&n| self.is_html(n, &local_name!("title"))) else {
            return String::new();
        };
        let text = self.child_text_content(t);
        let mut out = String::with_capacity(text.len());
        for word in text.split(['\t', '\n', '\x0c', '\r', ' ']).filter(|w| !w.is_empty()) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(word);
        }
        out
    }

    fn child_text_content(&self, id: NodeId) -> String {
        let mut s = String::new();
        let mut c = self.first_child(id);
        while let Some(n) = c {
            if let Some(t) = self.text(n) {
                s.push_str(t);
            }
            c = self.next_sibling(n);
        }
        s
    }

    // ----- text -----

    /// `node.textContent`.
    pub fn text_content(&self, id: NodeId) -> String {
        match &self.node(id).data {
            NodeData::Text(t) | NodeData::Comment(t) => t.to_string(),
            NodeData::ProcessingInstruction { data, .. } => data.to_string(),
            NodeData::Document | NodeData::Doctype { .. } => String::new(),
            _ => {
                let mut s = String::new();
                self.for_each_text(id, |t| s.push_str(t));
                s
            }
        }
    }

    /// Calls `f` with every text node under `id`, in tree order.
    #[inline]
    pub fn for_each_text(&self, id: NodeId, mut f: impl FnMut(&str)) {
        let mut node = self.first_child(id);
        while let Some(n) = node {
            if let NodeData::Text(t) = &self.node(n).data {
                f(t);
            }
            node = self.next_in(n, id);
        }
    }

    // ----- serialization -----

    /// `element.innerHTML`.
    pub fn inner_html(&self, id: NodeId) -> String {
        let mut out = String::new();
        let root = match self.element(id) {
            Some(ElementData { template_contents: Some(t), .. }) => *t,
            _ => id,
        };
        let mut child = self.first_child(root);
        while let Some(c) = child {
            self.serialize_into(c, &mut out);
            child = self.next_sibling(c);
        }
        out
    }

    /// `element.outerHTML`.
    pub fn outer_html(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.serialize_into(id, &mut out);
        out
    }

    /// Serializes `root` and its subtree (HTML fragment serialization algorithm).
    fn serialize_into(&self, root: NodeId, out: &mut String) {
        // Iterative to cope with very deep trees.
        let mut node = root;
        loop {
            let mut descend = None;
            match &self.node(node).data {
                NodeData::Element(e) => {
                    out.push('<');
                    out.push_str(&e.name.local);
                    for a in &e.attrs {
                        out.push(' ');
                        serialized_attr_name(&a.name, out);
                        out.push_str("=\"");
                        escape_attr(&a.value, out);
                        out.push('"');
                    }
                    out.push('>');
                    let void = e.name.ns == ns!(html) && is_void(&e.name.local);
                    if !void {
                        let content = e.template_contents.unwrap_or(node);
                        match self.first_child(content) {
                            Some(c) => descend = Some(c),
                            None => self.close_tag(node, out),
                        }
                    }
                }
                NodeData::Text(t) => {
                    let raw = self.parent(node).and_then(|p| self.html_name(p)).is_some_and(is_raw_text_parent);
                    if raw {
                        out.push_str(t);
                    } else {
                        escape_text(t, out);
                    }
                }
                NodeData::Comment(t) => {
                    out.push_str("<!--");
                    out.push_str(t);
                    out.push_str("-->");
                }
                NodeData::ProcessingInstruction { target, data } => {
                    let _ = write!(out, "<?{} {}>", &**target, &**data);
                }
                NodeData::Doctype { name, .. } => {
                    out.push_str("<!DOCTYPE ");
                    out.push_str(name);
                    out.push('>');
                }
                NodeData::Document | NodeData::Fragment { .. } => {
                    if let Some(c) = self.first_child(node) {
                        descend = Some(c);
                    }
                }
            }
            if let Some(c) = descend {
                node = c;
                continue;
            }
            // Move to the next sibling, closing finished parents.
            loop {
                if node == root {
                    return;
                }
                if let Some(s) = self.next_sibling(node) {
                    node = s;
                    break;
                }
                let parent = match self.parent(node) {
                    Some(p) => p,
                    None => return,
                };
                // Template contents: climb from the fragment back to the template.
                let parent = self.template_owner(parent).unwrap_or(parent);
                if self.is_element(parent) {
                    self.close_tag(parent, out);
                }
                node = parent;
            }
        }
    }

    fn template_owner(&self, fragment: NodeId) -> Option<NodeId> {
        match self.node(fragment).data {
            NodeData::Fragment { owner } => owner,
            _ => None,
        }
    }

    fn close_tag(&self, id: NodeId, out: &mut String) {
        if let Some(e) = self.element(id) {
            out.push_str("</");
            out.push_str(&e.name.local);
            out.push('>');
        }
    }
}

#[inline]
fn attr_name_is(name: &QualName, wanted: &str) -> bool {
    match &name.prefix {
        None => &*name.local == wanted,
        Some(prefix) => {
            let (p, l) = match wanted.split_once(':') {
                Some(x) => x,
                None => return false,
            };
            &**prefix == p && &*name.local == l
        }
    }
}

pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        let n = self.next?;
        self.next = self.doc.next_in(n, self.root);
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_serialize() {
        let doc = Document::parse(
            "<p class=a>x &amp; y<br><img src='a\"b'></p><!--c--><svg viewBox='0 0 1 1'><path/></svg>",
            None,
        );
        let body = doc.body().unwrap();
        assert_eq!(
            doc.inner_html(body),
            "<p class=\"a\">x &amp; y<br><img src=\"a&quot;b\"></p><!--c--><svg viewBox=\"0 0 1 1\"><path></path></svg>"
        );
        assert_eq!(doc.text_content(body), "x & y");
    }

    #[test]
    fn noscript_is_markup() {
        let doc = Document::parse("<p>x</p><noscript><img src=a.jpg></noscript>", None);
        let body = doc.body().unwrap();
        let ns = doc.last_child(body).unwrap();
        assert!(doc.is_html(doc.first_child(ns).unwrap(), &local_name!("img")));
    }

    #[test]
    fn mutation() {
        let mut doc = Document::parse("<div id=a><span>1</span><span>2</span></div>", None);
        let body = doc.body().unwrap();
        let div = doc.first_element_child(body).unwrap();
        let spans = doc.children(div);
        doc.append_child(body, spans[0]);
        assert_eq!(doc.inner_html(body), "<div id=\"a\"><span>2</span></div><span>1</span>");
        doc.rename(div, "P");
        let clone = doc.deep_clone(div);
        doc.replace(spans[0], clone);
        assert_eq!(doc.inner_html(body), "<p id=\"a\"><span>2</span></p><p id=\"a\"><span>2</span></p>");
        assert_eq!(doc.prev_sibling(div), None);
    }

    #[test]
    fn template_serialization() {
        let doc = Document::parse("<template><b>x</b></template><i>y</i>", None);
        let body = doc.body().unwrap();
        let head = doc.prev_element_sibling(body).unwrap();
        assert_eq!(doc.inner_html(head), "<template><b>x</b></template>");
        assert_eq!(doc.inner_html(body), "<i>y</i>");
    }

    #[test]
    fn svg_title_is_not_the_document_title() {
        let doc = Document::parse("<svg><title>Search</title></svg><p>x</p>", None);
        assert_eq!(doc.title(), "");
        let doc = Document::parse("<title> a \n b </title><svg><title>Search</title></svg>", None);
        assert_eq!(doc.title(), "a b");
    }

    #[test]
    fn clone_deeply_nested_templates() {
        let depth = 20_000;
        let html = format!("{}x{}", "<template>".repeat(depth), "</template>".repeat(depth));
        // Clone on a small stack: recursing once per template level would overflow it.
        let same = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                let mut doc = Document::parse(&html, None);
                let head = doc.first_element_child(doc.document_element().unwrap()).unwrap();
                let c = doc.deep_clone(head);
                doc.inner_html(head) == doc.inner_html(c)
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(same);
    }

    #[test]
    fn base_url() {
        let doc = Document::parse(
            "<base href='/foo/'><p>x</p>",
            Some(url::Url::parse("http://fakehost/test/page.html").unwrap()),
        );
        assert_eq!(doc.base_uri(), "http://fakehost/foo/");
        assert_eq!(doc.document_uri(), "http://fakehost/test/page.html");
    }
}
