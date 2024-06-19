use serde::{Serialize, Deserialize};

use crate::xml::{Xml};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ElementNodeData {
    pub attribute: String,
    pub is_id: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TextNodeData {
    pub is_informational: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NodeData {
    pub element_fields: Option<ElementNodeData>,
    pub text_fields: Option<TextNodeData>,
    pub name: String,
}

impl NodeData {
    pub fn value(&self, xml: &Xml) -> String {
        if let Some(text_fields) = &self.text_fields {
            return xml.to_string();
        }

        if let Some(element_fields) = &self.element_fields {
            return xml.get_attribute_value(&element_fields.attribute).unwrap();
        }

        panic!("NodeData neither has element or text fields!");
    }
}
