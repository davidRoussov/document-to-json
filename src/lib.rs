use tokio::runtime::Runtime;
use std::fs::{File};
use std::process;
use std::io::{Read};
use std::sync::{Arc};

mod error;
mod llm;
mod node_data;
mod node_data_structure;
mod utility;
mod xml_node;
mod config;
mod constants;
mod basis_node;
mod graph_node;
mod macros;
mod traversal;

use basis_node::{
    BasisNode
};
use graph_node::{
    GraphNode,
    Graph,
    absorb,
    cyclize,
    prune,
    interpret,
};
use xml_node::{XmlNode};
use error::{Errors};
use traversal::{Traversal};

pub fn normalize(text: String) -> Result<String, Errors> {
    log::trace!("In normalize");

    if text.trim().is_empty() {
        log::info!("Document not provided, aborting...");
        return Err(Errors::DocumentNotProvided);
    }

    return Runtime::new().unwrap().block_on(async {
        if utility::is_valid_xml(&text) {
            log::info!("Document is valid XML");

            let result = normalize_xml(&text).await?;

            return Ok(result);
        }

        if let Some(xml) = utility::string_to_xml(&text) {
            log::info!("Managed to convert string to XML");

            let result = normalize_xml(&xml).await?;

            return Ok(result);
        }

        Err(Errors::UnexpectedDocumentType)
    });
}

pub fn normalize_file(file_name: &str) -> Result<String, Errors> {
    log::trace!("In normalize_file");
    log::debug!("file_name: {}", file_name);

    let mut document = String::new();

    let mut file = File::open(file_name).unwrap_or_else(|err| {
        eprintln!("Failed to open file: {}", err);
        process::exit(1);
    });

    file.read_to_string(&mut document).unwrap_or_else(|err| {
        eprintln!("Failed to read file: {}", err);
        process::exit(1);
    });

    normalize(document)
}

pub async fn normalize_xml(xml: &str) -> Result<String, Errors> {
    log::trace!("In normalize_xml");

    let xml = utility::preprocess_xml(xml);
    log::info!("Done preprocessing XML");

    let input_tree: Graph<XmlNode> = graph_node::build_graph(xml.clone());
    let output_tree: Graph<XmlNode> = graph_node::build_graph(xml.clone());

    let basis_graph: Graph<BasisNode> = GraphNode::from_void();

    absorb(Arc::clone(&basis_graph), Arc::clone(&input_tree));
    log::info!("Done absorbing input tree into basis graph");
    read_lock!(basis_graph).debug_visualize("basis_graph_absorbed");

    cyclize(Arc::clone(&basis_graph));
    log::info!("Done cyclizing basis graph");
    read_lock!(basis_graph).debug_visualize("basis_graph_cyclized");

    prune(Arc::clone(&basis_graph));
    log::info!("Done pruning basis graph");
    read_lock!(basis_graph).debug_visualize("basis_graph_pruned");
    read_lock!(basis_graph).debug_statistics("basis_graph_pruned");

    log::info!("Interpreting basis graph...");
    interpret(Arc::clone(&basis_graph), Arc::clone(&output_tree)).await;
    log::info!("Done interpreting basis graph.");
    read_lock!(basis_graph).debug_visualize("basis_graph_interpreted");

    Traversal::from_tree(Arc::clone(&output_tree))
        .with_basis(Arc::clone(&basis_graph))
        .harvest()
}
