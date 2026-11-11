use quick_xml::{XmlVersion, events::Event};

/// An XML element by its local name, without namespaces.
#[derive(Debug, Default)]
pub struct Node {
    pub name: String,
    attributes: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The node at a `/`-separated path of local names.
    pub fn at(&self, path: &str) -> Option<&Node> {
        path.split('/')
            .try_fold(self, |node, name| node.child(name))
    }

    pub fn text(&self, path: &str) -> Option<String> {
        self.at(path)
            .map(|n| n.text.trim().to_owned())
            .filter(|t| !t.is_empty())
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or_default().to_owned()
}

pub fn parse(xml: &[u8]) -> Option<Node> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut stack = vec![Node::default()];
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => {
                let node = element(&e);
                stack.last_mut()?.children.push(node);
            }
            Event::End(_) => {
                let node = stack.pop()?;
                stack.last_mut()?.children.push(node);
            }
            Event::Text(t) => stack
                .last_mut()?
                .text
                .push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Event::GeneralRef(r) => {
                let resolved = match r.into_inner().as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                };
                stack.last_mut()?.text.push_str(resolved);
            }
            Event::CData(t) => stack.last_mut()?.text.push_str(&t.into_inner()),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let mut root = stack.pop()?;
    (stack.is_empty() && root.children.len() == 1).then(|| root.children.remove(0))
}

fn element(e: &quick_xml::events::BytesStart) -> Node {
    Node {
        name: local(e.name().as_ref()),
        attributes: e
            .attributes()
            .flatten()
            .map(|a| {
                (
                    local(a.key.as_ref()),
                    a.normalized_value(XmlVersion::Implicit1_0)
                        .map(|v| v.into_owned())
                        .unwrap_or_default(),
                )
            })
            .collect(),
        ..Node::default()
    }
}
