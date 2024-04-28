use sled::Db;
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use xmltree::{Element, XMLNode};
use std::cell::RefCell;
use std::rc::{Rc};
use uuid::Uuid;
use std::fs::OpenOptions;
use std::io::Write;
use tokio::time::{sleep, Duration};
use bincode::{serialize, deserialize};
use std::error::Error;
use std::collections::{HashMap, VecDeque};

use crate::node_data::{NodeData};
use crate::utility;
use crate::llm;

// echo -n "text_node" | sha256sum
const TEXT_NODE_HASH: &str = "40e215e7587a0edee158a67925057a5137f96c1c877fd3150f7d8760f866592e";



#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Node {
    pub id: String,
    pub hash: String,
    pub xml: String,
    pub is_structural: bool,
    pub parent: RefCell<Option<Rc<Node>>>,
    pub data: RefCell<Vec<NodeData>>,
    pub children: RefCell<Vec<Rc<Node>>>,
    pub complex_type_name: RefCell<Option<String>>,
}







pub fn build_tree(xml: String) -> Rc<Node> {
    let mut reader = std::io::Cursor::new(xml);
    let element = Element::parse(&mut reader).expect("Could not parse XML");

    Node::from_element(&element, None)
}

pub fn tree_to_xml(tree: Rc<Node>) -> String {
    let element = tree.to_element();

    utility::element_to_string(&element)
}










pub fn map_primitives(basis_tree: Rc<Node>, output_tree: Rc<Node>) -> HashMap<String, String> {
    unimplemented!()
}

pub fn node_data_to_hash_map(node_data: &RefCell<Vec<NodeData>>, output_tree: Rc<Node>) -> HashMap<String, String> {
    log::trace!("In node_data_to_hash_map");

    let mut values: HashMap<String, String> = HashMap::new();

    for item in node_data.borrow().iter() {
        if let Ok(output_tree_value) = utility::apply_xpath(&output_tree.xml, &item.xpath) {
            values.insert(item.name.clone(), output_tree_value.clone());
        } else {
            log::warn!("xpath from basis tree could not be applied to output tree");
        }
    }

    values
}

pub fn search_tree_by_lineage(mut tree: Rc<Node>, mut lineage: VecDeque<String>) -> Option<Rc<Node>> {
    log::trace!("In search_tree_by_lineage");

    //if let Some(hash) = lineage.pop_front() {
    //    if tree.hash != hash {
    //        return None;
    //    }
    //} else {
    //    return None;
    //}

    while let Some(hash) = lineage.pop_front() {
        log::trace!("hash: {}", hash);

        let node = tree
            .children
            .borrow()
            .iter()
            .find(|item| item.hash == hash)
            .cloned();

        if let Some(node) = node {
            tree = node;
        } else {
            return None;
        }
    }

    Some(tree)
}










pub fn bfs(node: Rc<Node>, visit: &mut dyn FnMut(&Rc<Node>)) {
    let mut queue = VecDeque::new();
    queue.push_back(node.clone());

    while let Some(current) = queue.pop_front() {
        visit(&current);

        for child in current.children.borrow().iter() {
            queue.push_back(child.clone());
        }
    }
}

pub fn dfs(node: Rc<Node>, visit: &mut dyn FnMut(&Rc<Node>)) {
    visit(&node);

    for child in node.children.borrow().iter() {
        dfs(child.clone(), visit);
    }
}

pub fn post_order_traversal(node: Rc<Node>, visit: &mut dyn FnMut(&Rc<Node>)) {
    for child in node.children.borrow().iter() {
        post_order_traversal(child.clone(), visit);
    }

    visit(&node);
}











pub async fn grow_tree(tree: Rc<Node>) {
    log::trace!("In grow_tree");

    let db = sled::open("src/database/hash_to_node_data").expect("Could not connect to datbase");

    let mut nodes: Vec<Rc<Node>> = Vec::new();

    post_order_traversal(tree.clone(), &mut |node: &Rc<Node>| {
        nodes.push(node.clone());
    });

    log::info!("There are {} nodes to be evaluated", nodes.len());

    for (index, node) in nodes.iter().enumerate() {
        log::info!("Interpreting node #{} out of {}", index + 1, nodes.len());
        node.update_node_data(&db).await;
        node.update_node_data_values();
        node.interpret_node_data(&db).await;
        sleep(Duration::from_secs(1)).await;
    }
}

pub fn prune_tree(tree: Rc<Node>) {
    log::trace!("In prune_tree");

    bfs(Rc::clone(&tree), &mut |node: &Rc<Node>| {
        loop {

            if node.parent.borrow().is_none() {
                break;
            }

            let mut children_borrow = node.children.borrow();
            log::debug!("Node has {} children", children_borrow.len());
            
            let twins: Option<(Rc<Node>, Rc<Node>)> = children_borrow.iter()
                .find_map(|child| {
                    children_borrow.iter()
                        .find(|&sibling| sibling.id != child.id && sibling.hash == child.hash && sibling.parent.borrow().is_some())
                        .map(|sibling| (Rc::clone(child), Rc::clone(sibling)))
                });

            drop(children_borrow);

            if let Some(twins) = twins {
                log::trace!("Pruning nodes with ids: {} and {} with hash {}", twins.0.id, twins.1.id, twins.0.hash);
                merge_nodes(node.clone(), twins);
            } else {
                break;
            }
        }
    });
}

pub fn merge_nodes(parent: Rc<Node>, nodes: (Rc<Node>, Rc<Node>)) {
    log::trace!("In merge_nodes");

    *nodes.1.parent.borrow_mut() = None;

    for child in nodes.1.children.borrow_mut().iter() {
        *child.parent.borrow_mut() = Some(nodes.0.clone()).into();
        nodes.0.children.borrow_mut().push(child.clone());
    }

    parent.children.borrow_mut().retain(|child| child.id != nodes.1.id);
}

pub fn absorb_tree(recipient: Rc<Node>, donor: Rc<Node>) {
    log::trace!("In absorb_tree");

    let recipient_child = {
        recipient.children.borrow().iter().find(|item| item.hash == donor.hash).cloned()
    };

   if let Some(recipient_child) = recipient_child {
       log::trace!("Donor and recipient node have the same hash");

       if recipient_child.subtree_hash() == donor.subtree_hash() {
           log::trace!("Donor and recipient child subtree hashes match");
           return;
       } else {
           log::trace!("Donor and recipient child have differing subtree hashes");
           let donor_children = donor.children.borrow().clone();

           for donor_child in donor_children.iter() {
               absorb_tree(recipient_child.clone(), donor_child.clone());
           }
       }
    } else {
        log::trace!("Donor and recipient subtrees incompatible. Adopting donor node...");

        //recipient.adopt_child(donor);

        *donor.parent.borrow_mut() = Some(recipient.clone());
        recipient.children.borrow_mut().push(donor);
    }
}














pub fn log_tree(tree: Rc<Node>, title: &str) {

    let xml = tree_to_xml(tree.clone());
    let xml_file_name = format!("tree_{}.xml", tree.ancestry_hash());

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("./debug/trees")
        .expect("Could not open file");

    let divider = std::iter::repeat("*").take(100).collect::<String>();
    let text = format!(
        "\n\n{} {}\n",
        divider,
        title
    );

    writeln!(file, "{}", text).expect("Could not write to file");

    let mut node_count = 0;

    bfs(tree.clone(), &mut |node: &Rc<Node>| {
        node_count = node_count + 1;

        let divider = std::iter::repeat("-").take(50).collect::<String>();
        let text = format!(
            "\nID: {}\nHASH: {}\nXML: {}\nSUBTREE HASH: {}\nANCESTOR HASH: {}\nCOMPLEX TYPE NAME: {:?}\n",
            node.id,
            node.hash,
            node.xml,
            node.subtree_hash(),
            node.ancestry_hash(),
            node.complex_type_name
        );

        let mut node_data_text = String::from("");

        for d in node.data.borrow().iter() {
            node_data_text = node_data_text + format!(r##"
                xpath: {},
                name: {},
                value: {:?}
            "##, d.xpath, d.name, d.value).as_str();
        }

        let text = format!("\n{}{}{}{}\n", divider, text, node_data_text, divider);

        writeln!(file, "{}", text).expect("Could not write to file");
    });

    writeln!(file, "node count: {}", node_count).expect("Could not write to file");
}
























impl Node {
    pub fn from_void() -> Rc<Self> {
        let tag = String::from("<>");
        let hash = utility::hash_text(tag.clone());

        Rc::new(Node {
            id: Uuid::new_v4().to_string(),
            hash: hash,
            xml: tag.clone(),
            is_structural: true,
            parent: None.into(),
            data: RefCell::new(Vec::new()),
            children: RefCell::new(vec![]),
            complex_type_name: RefCell::new(None),
        })
    }

    pub fn from_text(text: String, parent: Option<Rc<Node>>) -> Rc<Self> {
        Rc::new(Node {
            id: Uuid::new_v4().to_string(),
            hash: TEXT_NODE_HASH.to_string(),
            xml: text,
            is_structural: false,
            parent: parent.into(),
            data: RefCell::new(Vec::new()),
            children: RefCell::new(vec![]),
            complex_type_name: RefCell::new(None),
        })
    }

    pub fn from_element(element: &Element, parent: Option<Rc<Node>>) -> Rc<Self> {
        let tag = element.name.clone();
        let xml = utility::get_element_xml(&element);
        let element_fields = element.attributes.keys().cloned().collect();
        let is_structural = element.attributes.len() == 0;

        let node = Rc::new(Node {
            id: Uuid::new_v4().to_string(),
            hash: utility::generate_element_node_hash(tag.clone(), element_fields),
            xml: xml,
            is_structural: is_structural,
            parent: parent.into(),
            data: RefCell::new(Vec::new()),
            children: RefCell::new(vec![]),
            complex_type_name: RefCell::new(None),
        });

       let children_nodes: Vec<Rc<Node>> = element.children.iter().filter_map(|child| {
            match child {
                XMLNode::Element(child_element) => Some(Node::from_element(&child_element, Some(Rc::clone(&node)))),
                XMLNode::Text(child_text) => Some(Node::from_text(child_text.to_string(), Some(Rc::clone(&node)))),
                _ => None,
            }
        }).collect();

        node.children.borrow_mut().extend(children_nodes);

        node
    }

    pub fn to_element(&self) -> Element {
        unimplemented!()
        //let mut element = Element::new(&self.tag);

        //for child in self.children.borrow().iter() {
        //    element.children.push(
        //        XMLNode::Element(child.to_element())
        //    );
        //}

        //element
    }
}


















pub fn get_lineage(node: Rc<Node>) -> VecDeque<String> {
    let mut lineage = VecDeque::new();
    lineage.push_back(node.hash.clone());

    let mut current_parent = node.parent.borrow().clone();

    while let Some(parent) = current_parent {
        lineage.push_front(parent.hash.clone());

        current_parent = {
            let node_ref = parent.parent.borrow();
            node_ref.as_ref().map(|node| node.clone())
        };
    }

    lineage
}

impl Node {

    pub fn ancestry_hash(&self) -> String {
        let mut hasher = Sha256::new();

        let mut hasher_items = Vec::new();
        hasher_items.push(self.hash.clone());

        if let Some(parent) = self.parent.borrow().as_ref() {
            hasher_items.push(
                parent.ancestry_hash()
            );
        }

        hasher_items.sort();
        hasher.update(hasher_items.join(""));

        format!("{:x}", hasher.finalize())
    }

    pub fn subtree_hash(&self) -> String {
        let mut hasher = Sha256::new();

        let mut hasher_items = Vec::new();
        hasher_items.push(self.hash.clone());

        for child in self.children.borrow().iter() {
            hasher_items.push(child.subtree_hash());
        }

        hasher_items.sort();
        hasher.update(hasher_items.join(""));

        format!("{:x}", hasher.finalize())
    }
}


















impl Node {
    pub fn should_update_node_data(&self) -> bool {
        log::trace!("In should_update_node_data");

        !self.is_structural
    }

    pub fn should_interpret_node_data(&self) -> bool {
        log::trace!("In should_interpret_node_data");
        
        // Do not give a node a type if:
        // * It's a leaf node
        // * It and all children are structural nodes

        let is_leaf = self.children.borrow().is_empty();

        let is_structural = self.children.borrow().iter().fold(
            self.is_structural,
            |acc, item| {
                acc && item.is_structural
            }
        );

        is_leaf || is_structural
    }

    pub fn should_propagate_node_interpretation(&self) -> Option<String> {
        log::trace!("In should_propagate_node_interpretation");

        // We should propagate descendant complex type to parent if:
        // Node only has one non-structural child
        // TODO: what if node and all children except one are structural, and structural node is leaf node?

        let structural_count: u16 = self.children.borrow().iter().fold(
            0 as u16,
            |acc, item| {
                acc + item.is_structural as u16
            }
        );

        if self.is_structural && structural_count == 1 {

            let sole_non_structural_node: Rc<Node> = self.children.borrow().iter().find(|item| {
                !item.is_structural
            }).unwrap().clone();

            let complex_type_name = sole_non_structural_node.complex_type_name.borrow().clone().unwrap();

            return Some(complex_type_name);
        }

        None
    }

    pub fn should_classically_update_node_data(&self) -> Option<Vec<NodeData>> {
        log::trace!("In should_classically_update_node_data");

        // * We don't need to consult an LLM to interpret text nodes

        if self.hash == TEXT_NODE_HASH {
            let node_data = NodeData {
                name: String::from("text"),
                value: NodeDataValue {
                    text: self.xml.clone(),
                },
                xpath: String::from(""),
            };

            return Some(vec![node_data]);
        }

        None
    }

    pub async fn update_node_data(&self, db: &Db) {
        log::trace!("In update_node_data");

        if !self.should_update_node_data() {
            log::info!("Not updating this node");
            *self.data.borrow_mut() = Vec::new();
            return;
        }

        if let Some(node_data) = get_node_data(&db, &self.hash).expect("Could not get node data from database") {
            log::info!("Cache hit!");
            *self.data.borrow_mut() = node_data.clone();
        } else {
            log::info!("Cache miss!");
            let llm_node_data: Vec<NodeData> = llm::generate_node_data(self.xml.clone()).await.expect("LLM unable to generate node data");
            *self.data.borrow_mut() = llm_node_data.clone();

            store_node_data(&db, &self.hash, llm_node_data.clone()).expect("Unable to persist node data to database");
        }
    }

    pub fn update_node_data_values(&self) {
        unimplemented!()
        //let mut data = self.data.borrow_mut();

        //for item in data.iter_mut() {
        //    if let Ok(result) = utility::apply_xpath(&self.xml, &item.xpath) {
        //        log::trace!("xpath success match: {}", result);
        //        item.value = Some(result.clone());
        //    } else {
        //        log::warn!("Could not apply xpath: {} to node with id: {}", &item.xpath, self.id);
        //        item.value = None;
        //    }
        //}
    }

    pub async fn interpret_node_data(&self, db: &Db) {
        log::trace!("In interpret_node_data");

        if !self.should_interpret_node_data() {
            log::info!("Not interpreting this node");
            *self.complex_type_name.borrow_mut() = None.into();
            return;
        }
        
        if let Some(propagated_complex_type) = self.should_propagate_node_interpretation() {
            log::info!("Propagating node interpretation");
            *self.complex_type_name.borrow_mut() = Some(propagated_complex_type);
            return;
        }

        log::info!("Consulting LLM for node interpretation...");

        let subtree_hash = &self.subtree_hash();

        if let Some(complex_type) = get_node_complex_type(&db, subtree_hash).expect("Could not get node complex type from database") {
            log::info!("Cache hit!");
            *self.complex_type_name.borrow_mut() = Some(complex_type.clone());
        } else {
            log::info!("Cache miss!");


            // TODO: feel this belongs in llm module
            let fields = self.children.borrow().iter().fold(
                node_data_to_string(self.data.borrow().clone()),
                |acc, item| {
                    if let Some(complex_type_name) = item.complex_type_name.borrow().clone() {
                        format!("{}\n{}: {}", acc, uncapitalize(&complex_type_name), &complex_type_name)
                    } else {
                        format!("{}\n{}", acc, node_data_to_string(item.data.borrow().clone()))
                    }
                }
            );



            //let llm_type_name: String = llm::interpret_node(&Rc::new(self.clone())).await
            //    .expect("Could not interpret node");
            let llm_type_name: String = llm::interpret_node(fields).await
                .expect("Could not interpret node");

            *self.complex_type_name.borrow_mut() = Some(llm_type_name.clone()).into();

            store_node_complex_type(&db, subtree_hash, &llm_type_name).expect("Unable to persist complex type to database");
        }
    }
}

fn node_data_to_string(node_data: Vec<NodeData>) -> String {
    node_data.iter().fold(String::from(""), |acc, item| {
        format!("{}\n{}", acc, item.name)
    })
}

fn uncapitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_lowercase().collect::<String>() + chars.as_str(),
    }
}















pub fn store_node_data(db: &Db, key: &str, nodes: Vec<NodeData>) -> Result<(), Box<dyn Error>> {
    let serialized_nodes = serialize(&nodes)?;
    db.insert(key, serialized_nodes)?;
    Ok(())
}

pub fn get_node_data(db: &Db, key: &str) -> Result<Option<Vec<NodeData>>, Box<dyn Error>> {
    match db.get(key)? {
        Some(serialized_nodes) => {
            let nodes_data: Vec<NodeData> = deserialize(&serialized_nodes)?;
            Ok(Some(nodes_data))
        },
        None => Ok(None),
    }
} 

pub fn store_node_complex_type(db: &Db, key: &str, complex_type: &str) -> Result<(), Box<dyn Error>> {
    db.insert(key, complex_type)?;
    Ok(())
}

pub fn get_node_complex_type(db: &Db, key: &str) -> Result<Option<String>, Box<dyn Error>> {
    match db.get(key)? {
        Some(iv) => {
            let complex_type = String::from_utf8(iv.to_vec())?;
            Ok(Some(complex_type))
        },
        None => Ok(None),
    }
} 
