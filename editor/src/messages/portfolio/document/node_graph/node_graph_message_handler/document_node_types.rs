use super::{node_properties, FrontendGraphDataType, FrontendNodeType};
use crate::consts::{DEFAULT_FONT_FAMILY, DEFAULT_FONT_STYLE};
use crate::messages::layout::utility_types::widget_prelude::*;
use crate::node_graph_executor::NodeGraphExecutor;

use graph_craft::concrete;
use graph_craft::document::value::*;
use graph_craft::document::*;
use graph_craft::imaginate_input::ImaginateSamplingMethod;
use graph_craft::NodeIdentifier;
#[cfg(feature = "gpu")]
use graphene_core::application_io::SurfaceHandle;
use graphene_core::raster::brush_cache::BrushCache;
use graphene_core::raster::{BlendMode, Color, Image, ImageFrame, LuminanceCalculation, RedGreenBlue, RelativeAbsolute, SelectiveColorChoice};
use graphene_core::text::Font;
use graphene_core::vector::VectorData;
use graphene_core::*;

#[cfg(feature = "gpu")]
use gpu_executor::*;
use graphene_std::wasm_application_io::WasmEditorApi;
use once_cell::sync::Lazy;
use std::collections::VecDeque;
#[cfg(feature = "gpu")]
use wgpu_executor::WgpuExecutor;

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct DocumentInputType {
	pub name: &'static str,
	pub data_type: FrontendGraphDataType,
	pub default: NodeInput,
}

impl DocumentInputType {
	pub fn new(name: &'static str, data_type: FrontendGraphDataType, default: NodeInput) -> Self {
		Self { name, data_type, default }
	}

	pub fn value(name: &'static str, tagged_value: TaggedValue, exposed: bool) -> Self {
		let data_type = FrontendGraphDataType::with_tagged_value(&tagged_value);
		let default = NodeInput::value(tagged_value, exposed);
		Self { name, data_type, default }
	}

	pub const fn none() -> Self {
		Self {
			name: "None",
			data_type: FrontendGraphDataType::General,
			default: NodeInput::value(TaggedValue::None, false),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct DocumentOutputType {
	pub name: &'static str,
	pub data_type: FrontendGraphDataType,
}

impl DocumentOutputType {
	pub const fn new(name: &'static str, data_type: FrontendGraphDataType) -> Self {
		Self { name, data_type }
	}
}

pub struct NodePropertiesContext<'a> {
	pub persistent_data: &'a crate::messages::portfolio::utility_types::PersistentData,
	pub document: &'a document_legacy::document::Document,
	pub responses: &'a mut VecDeque<crate::messages::prelude::Message>,
	pub layer_path: &'a [document_legacy::LayerId],
	pub nested_path: &'a [NodeId],
	pub executor: &'a mut NodeGraphExecutor,
	pub network: &'a NodeNetwork,
}

#[derive(Clone)]
pub enum NodeImplementation {
	ProtoNode(NodeIdentifier),
	DocumentNode(NodeNetwork),
	Extract,
}

impl Default for NodeImplementation {
	fn default() -> Self {
		Self::ProtoNode(NodeIdentifier::new("graphene_core::ops::IdNode"))
	}
}

impl NodeImplementation {
	pub fn proto(name: &'static str) -> Self {
		Self::ProtoNode(NodeIdentifier::new(name))
	}
}

#[derive(Clone)]
pub struct DocumentNodeType {
	pub name: &'static str,
	pub category: &'static str,
	pub identifier: NodeImplementation,
	pub inputs: Vec<DocumentInputType>,
	pub outputs: Vec<DocumentOutputType>,
	pub primary_output: bool,
	pub properties: fn(&DocumentNode, NodeId, &mut NodePropertiesContext) -> Vec<LayoutGroup>,
}

impl Default for DocumentNodeType {
	fn default() -> Self {
		Self {
			name: Default::default(),
			category: Default::default(),
			identifier: Default::default(),
			inputs: Default::default(),
			outputs: Default::default(),
			primary_output: Default::default(),
			properties: node_properties::no_properties,
		}
	}
}

// We use the once cell for lazy initialization to avoid the overhead of reconstructing the node list every time.
// TODO: make document nodes not require a `'static` lifetime to avoid having to split the construction into const and non-const parts.
static DOCUMENT_NODE_TYPES: once_cell::sync::Lazy<Vec<DocumentNodeType>> = once_cell::sync::Lazy::new(static_nodes);

// TODO: Dynamic node library
fn static_nodes() -> Vec<DocumentNodeType> {
	vec![
		DocumentNodeType {
			name: "Boolean",
			category: "Inputs",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType::value("Bool", TaggedValue::Bool(true), false)],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::Boolean)],
			properties: node_properties::boolean_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Value",
			category: "Inputs",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType::value("Value", TaggedValue::F32(0.), false)],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::Number)],
			properties: node_properties::value_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Color",
			category: "Inputs",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType::value("Value", TaggedValue::OptionalColor(None), false)],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::Color)],
			properties: node_properties::color_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Identity",
			category: "Structural",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType {
				name: "In",
				data_type: FrontendGraphDataType::General,
				default: NodeInput::value(TaggedValue::None, true),
			}],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::General)],
			properties: |_document_node, _node_id, _context| node_properties::string_properties("The identity node simply returns the input"),
			..Default::default()
		},
		DocumentNodeType {
			name: "Monitor",
			category: "Structural",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType {
				name: "In",
				data_type: FrontendGraphDataType::General,
				default: NodeInput::value(TaggedValue::None, true),
			}],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::General)],
			properties: |_document_node, _node_id, _context| node_properties::string_properties("The Monitor node stores the value of its last evaluation"),
			..Default::default()
		},
		DocumentNodeType {
			name: "Layer",
			category: "General",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0; 8],
				outputs: vec![NodeOutput::new(1, 0)],
				nodes: [
					(
						0,
						DocumentNode {
							inputs: vec![
								NodeInput::Network(concrete!(graphene_core::vector::VectorData)),
								NodeInput::Network(concrete!(String)),
								NodeInput::Network(concrete!(BlendMode)),
								NodeInput::Network(concrete!(f32)),
								NodeInput::Network(concrete!(bool)),
								NodeInput::Network(concrete!(bool)),
								NodeInput::Network(concrete!(bool)),
								NodeInput::Network(concrete!(graphene_core::GraphicGroup)),
							],
							implementation: DocumentNodeImplementation::proto("graphene_core::ConstructLayerNode<_, _, _, _, _, _, _>"),
							..Default::default()
						},
					),
					// The monitor node is used to display a thumbnail in the UI.
					(
						1,
						DocumentNode {
							inputs: vec![NodeInput::node(0, 0)],
							implementation: DocumentNodeImplementation::proto("graphene_core::memo::MonitorNode<_>"),
							..Default::default()
						},
					),
				]
				.into(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType::value("Vector Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true),
				DocumentInputType::value("Name", TaggedValue::String(String::new()), false),
				DocumentInputType::value("Blend Mode", TaggedValue::BlendMode(BlendMode::Normal), false),
				DocumentInputType::value("Opacity", TaggedValue::F32(100.), false),
				DocumentInputType::value("Visible", TaggedValue::Bool(true), false),
				DocumentInputType::value("Locked", TaggedValue::Bool(false), false),
				DocumentInputType::value("Collapsed", TaggedValue::Bool(false), false),
				DocumentInputType::value("Stack", TaggedValue::GraphicGroup(GraphicGroup::EMPTY), true),
			],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::GraphicGroup)],
			properties: node_properties::layer_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Artboard",
			category: "General",
			identifier: NodeImplementation::proto("graphene_core::ConstructArtboardNode<_, _, _, _>"),
			inputs: vec![
				DocumentInputType::value("Graphic Group", TaggedValue::GraphicGroup(GraphicGroup::EMPTY), true),
				DocumentInputType::value("Location", TaggedValue::IVec2(glam::IVec2::ZERO), false),
				DocumentInputType::value("Dimensions", TaggedValue::IVec2(glam::IVec2::new(1920, 1080)), false),
				DocumentInputType::value("Background", TaggedValue::Color(Color::WHITE), false),
				DocumentInputType::value("Clip", TaggedValue::Bool(false), false),
			],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::Artboard)],
			properties: node_properties::artboard_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Downres",
			category: "Raster",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0],
				outputs: vec![NodeOutput::new(1, 0)],
				nodes: [
					DocumentNode {
						name: "Downres".to_string(),
						inputs: vec![NodeInput::Network(concrete!(ImageFrame<Color>))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_std::raster::DownresNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
					// We currently just clone by default
					/*DocumentNode {
						name: "Clone".to_string(),
						inputs: vec![NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::CloneNode<_>")),
						..Default::default()
					},*/
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), false)],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: |_document_node, _node_id, _context| node_properties::string_properties("Downres the image to a lower resolution"),
			..Default::default()
		},
		// DocumentNodeType {
		// 	name: "Input Frame",
		// 	category: "Ignore",
		// 	identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
		// 	inputs: vec![DocumentInputType {
		// 		name: "In",
		// 		data_type: FrontendGraphDataType::Raster,
		// 		default: NodeInput::Network,
		// 	}],
		// 	outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::Raster)],
		// 	properties: node_properties::input_properties,
		// },
		DocumentNodeType {
			name: "Input Frame",
			category: "Ignore",
			identifier: NodeImplementation::proto("graphene_core::ExtractImageFrame"),
			inputs: vec![DocumentInputType {
				name: "In",
				data_type: FrontendGraphDataType::General,
				default: NodeInput::Network(concrete!(WasmEditorApi)),
			}],
			outputs: vec![DocumentOutputType {
				name: "Image Frame",
				data_type: FrontendGraphDataType::Raster,
			}],
			properties: node_properties::input_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Load Image",
			category: "Structural",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0, 0],
				outputs: vec![NodeOutput::new(1, 0)],
				nodes: [
					DocumentNode {
						name: "Load Resource".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi)), NodeInput::Network(concrete!(String))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_std::wasm_application_io::LoadResourceNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "Decode Image".to_string(),
						inputs: vec![NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_std::wasm_application_io::DecodeImageNode")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "api",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
				DocumentInputType {
					name: "path",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::String("graphite:null".to_string()), false),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "Image Frame",
				data_type: FrontendGraphDataType::Raster,
			}],
			properties: node_properties::load_image_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Create Canvas",
			category: "Structural",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0],
				outputs: vec![NodeOutput::new(1, 0)],
				nodes: [
					DocumentNode {
						name: "Create Canvas".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_std::wasm_application_io::CreateSurfaceNode")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![DocumentInputType {
				name: "In",
				data_type: FrontendGraphDataType::General,
				default: NodeInput::Network(concrete!(WasmEditorApi)),
			}],
			outputs: vec![DocumentOutputType {
				name: "Canvas",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		DocumentNodeType {
			name: "Draw Canvas",
			category: "Structural",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0, 2],
				outputs: vec![NodeOutput::new(3, 0)],
				nodes: [
					DocumentNode {
						name: "Convert Image Frame".to_string(),
						inputs: vec![NodeInput::Network(concrete!(ImageFrame<Color>))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, ImageFrame<SRGBA8>>")),
						..Default::default()
					},
					DocumentNode {
						name: "Create Canvas".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_std::wasm_application_io::CreateSurfaceNode")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
					DocumentNode {
						name: "Draw Canvas".to_string(),
						inputs: vec![NodeInput::node(0, 0), NodeInput::node(2, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_std::wasm_application_io::DrawImageFrameNode<_>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::Raster,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "Canvas",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		DocumentNodeType {
			name: "Begin Scope",
			category: "Ignore",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0],
				outputs: vec![NodeOutput::new(1, 0), NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "SetNode".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::SomeNode")),
						..Default::default()
					},
					DocumentNode {
						name: "LetNode".to_string(),
						inputs: vec![NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::LetNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "RefNode".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::lambda(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::RefNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),

				..Default::default()
			}),
			inputs: vec![DocumentInputType {
				name: "In",
				data_type: FrontendGraphDataType::Raster,
				default: NodeInput::Network(concrete!(WasmEditorApi)),
			}],
			outputs: vec![
				DocumentOutputType {
					name: "Scope",
					data_type: FrontendGraphDataType::General,
				},
				DocumentOutputType {
					name: "Binding",
					data_type: FrontendGraphDataType::Raster,
				},
			],
			properties: |_document_node, _node_id, _context| node_properties::string_properties("Binds the input in a local scope as a variable"),
			..Default::default()
		},
		DocumentNodeType {
			name: "End Scope",
			category: "Ignore",
			identifier: NodeImplementation::proto("graphene_core::memo::EndLetNode<_>"),
			inputs: vec![
				DocumentInputType {
					name: "Scope",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
				DocumentInputType {
					name: "Data",
					data_type: FrontendGraphDataType::Raster,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "Frame",
				data_type: FrontendGraphDataType::Raster,
			}],
			properties: |_document_node, _node_id, _context| node_properties::string_properties("The graph's output is drawn in the layer"),
			..Default::default()
		},
		DocumentNodeType {
			name: "Output",
			category: "Ignore",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType {
				name: "Output",
				data_type: FrontendGraphDataType::Raster,
				default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
			}],
			outputs: vec![],
			properties: node_properties::output_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Image Frame",
			category: "General",
			identifier: NodeImplementation::proto("graphene_std::raster::ImageFrameNode<_, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::Image(Image::empty()), true),
				DocumentInputType::value("Transform", TaggedValue::DAffine2(DAffine2::IDENTITY), true),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: |_document_node, _node_id, _context| node_properties::string_properties("Creates an embedded image with the given transform"),
			..Default::default()
		},
		DocumentNodeType {
			name: "Mask",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_std::raster::MaskImageNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Stencil", TaggedValue::ImageFrame(ImageFrame::empty()), true),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::mask_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Insert Channel",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_std::raster::InsertChannelNode<_, _, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Insertion", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Replace", TaggedValue::RedGreenBlue(RedGreenBlue::Red), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::insert_channel_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Combine Channels",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_std::raster::CombineChannelsNode"),
			inputs: vec![
				DocumentInputType::value("None", TaggedValue::None, false),
				DocumentInputType::value("Red", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Green", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Blue", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Alpha", TaggedValue::ImageFrame(ImageFrame::empty()), true),
			],
			outputs: vec![DocumentOutputType {
				name: "Image",
				data_type: FrontendGraphDataType::Raster,
			}],
			..Default::default()
		},
		DocumentNodeType {
			name: "Blend",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::BlendNode<_, _, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Second", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("BlendMode", TaggedValue::BlendMode(BlendMode::Normal), false),
				DocumentInputType::value("Opacity", TaggedValue::F32(100.), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::blend_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Levels",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::LevelsNode<_, _, _, _, _>"),
			inputs: vec![
				DocumentInputType {
					name: "Image",
					data_type: FrontendGraphDataType::Raster,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
				DocumentInputType {
					name: "Shadows",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(0.), false),
				},
				DocumentInputType {
					name: "Midtones",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(50.), false),
				},
				DocumentInputType {
					name: "Highlights",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(100.), false),
				},
				DocumentInputType {
					name: "Output Minimums",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(0.), false),
				},
				DocumentInputType {
					name: "Output Maximums",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(100.), false),
				},
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::levels_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Grayscale",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::GrayscaleNode<_, _, _, _, _, _, _>"),
			inputs: vec![
				DocumentInputType {
					name: "Image",
					data_type: FrontendGraphDataType::Raster,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
				DocumentInputType {
					name: "Tint",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::Color(Color::BLACK), false),
				},
				DocumentInputType {
					name: "Reds",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(40.), false),
				},
				DocumentInputType {
					name: "Yellows",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(60.), false),
				},
				DocumentInputType {
					name: "Greens",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(40.), false),
				},
				DocumentInputType {
					name: "Cyans",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(60.), false),
				},
				DocumentInputType {
					name: "Blues",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(20.), false),
				},
				DocumentInputType {
					name: "Magentas",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::F32(80.), false),
				},
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::grayscale_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Color Channel",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType::value("Channel", TaggedValue::RedGreenBlue(RedGreenBlue::Red), false)],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::General)],
			properties: node_properties::color_channel_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Blend Mode",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType::value("Mode", TaggedValue::BlendMode(BlendMode::Normal), false)],
			outputs: vec![DocumentOutputType::new("Out", FrontendGraphDataType::General)],
			properties: node_properties::blend_mode_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Luminance",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::LuminanceNode<_>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Luminance Calc", TaggedValue::LuminanceCalculation(LuminanceCalculation::SRGB), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::luminance_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Extract Channel",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::ExtractChannelNode<_>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("From", TaggedValue::RedGreenBlue(RedGreenBlue::Red), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::extract_channel_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Extract Alpha",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::ExtractAlphaNode<>"),
			inputs: vec![DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true)],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Extract Opaque",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::ExtractOpaqueNode<>"),
			inputs: vec![DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true)],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Split Channels",
			category: "Image Adjustments",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0],
				outputs: vec![NodeOutput::new(4, 0), NodeOutput::new(1, 0), NodeOutput::new(2, 0), NodeOutput::new(3, 0), NodeOutput::new(4, 0)],
				nodes: [
					DocumentNode {
						name: "Identity".to_string(),
						inputs: vec![NodeInput::Network(concrete!(ImageFrame<Color>))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IdNode")),
						..Default::default()
					},
					DocumentNode {
						name: "RedNode".to_string(),
						inputs: vec![NodeInput::node(0, 0), NodeInput::value(TaggedValue::RedGreenBlue(RedGreenBlue::Red), false)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::raster::ExtractChannelNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "GreenNode".to_string(),
						inputs: vec![NodeInput::node(0, 0), NodeInput::value(TaggedValue::RedGreenBlue(RedGreenBlue::Green), false)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::raster::ExtractChannelNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "BlueNode".to_string(),
						inputs: vec![NodeInput::node(0, 0), NodeInput::value(TaggedValue::RedGreenBlue(RedGreenBlue::Blue), false)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::raster::ExtractChannelNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "AlphaNode".to_string(),
						inputs: vec![NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::raster::ExtractAlphaNode<>")),
						..Default::default()
					},
					DocumentNode {
						name: "EmptyOutput".to_string(),
						inputs: vec![NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), false)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IdNode")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),

				..Default::default()
			}),
			inputs: vec![DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true)],
			outputs: vec![
				DocumentOutputType::new("Empty", FrontendGraphDataType::Raster),
				DocumentOutputType::new("Red", FrontendGraphDataType::Raster),
				DocumentOutputType::new("Green", FrontendGraphDataType::Raster),
				DocumentOutputType::new("Blue", FrontendGraphDataType::Raster),
				DocumentOutputType::new("Alpha", FrontendGraphDataType::Raster),
			],
			primary_output: false,
			..Default::default()
		},
		DocumentNodeType {
			name: "Brush",
			category: "Brush",
			identifier: NodeImplementation::proto("graphene_std::brush::BrushNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Background", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Bounds", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Trace", TaggedValue::BrushStrokes(Vec::new()), false),
				DocumentInputType::value("Cache", TaggedValue::BrushCache(BrushCache::new_proto()), false),
			],
			outputs: vec![DocumentOutputType {
				name: "Image",
				data_type: FrontendGraphDataType::Raster,
			}],
			..Default::default()
		},
		DocumentNodeType {
			name: "Extract Vector Points",
			category: "Brush",
			identifier: NodeImplementation::proto("graphene_std::brush::VectorPointsNode"),
			inputs: vec![DocumentInputType::value("VectorData", TaggedValue::VectorData(VectorData::empty()), true)],
			outputs: vec![DocumentOutputType {
				name: "Vector Points",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		DocumentNodeType {
			name: "Memoize",
			category: "Structural",
			identifier: NodeImplementation::proto("graphene_core::memo::MemoNode<_, _>"),
			inputs: vec![
				DocumentInputType::value("ShortCircut", TaggedValue::None, false),
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Image",
			category: "Ignore",
			identifier: NodeImplementation::proto("graphene_core::ops::IdNode"),
			inputs: vec![DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), false)],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: |_document_node, _node_id, _context| node_properties::string_properties("A bitmap image embedded in this node"),
			..Default::default()
		},
		DocumentNodeType {
			name: "Ref",
			category: "Structural",
			identifier: NodeImplementation::proto("graphene_core::memo::MemoNode<_, _>"),
			inputs: vec![DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true)],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "Uniform",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 0],
				outputs: vec![NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Create Uniform".to_string(),
						inputs: vec![NodeInput::Network(generic!(T)), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::UniformNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::F32(0.), true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "Uniform",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "Storage",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 0],
				outputs: vec![NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Create Storage".to_string(),
						inputs: vec![NodeInput::Network(concrete!(Vec<u8>)), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::StorageNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "Storage",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "CreateOutputBuffer",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 1, 0],
				outputs: vec![NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Create Output Buffer".to_string(),
						inputs: vec![NodeInput::Network(concrete!(usize)), NodeInput::node(0, 0), NodeInput::Network(concrete!(Type))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::CreateOutputBufferNode<_, _>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "OutputBuffer",
				data_type: FrontendGraphDataType::General,
			}],
			properties: node_properties::input_properties,
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "CreateComputePass",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 0, 1, 1],
				outputs: vec![NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Create Compute Pass".to_string(),
						inputs: vec![
							NodeInput::Network(concrete!(gpu_executor::PipelineLayout<WgpuExecutor>)),
							NodeInput::node(0, 0),
							NodeInput::Network(concrete!(ShaderInput<WgpuExecutor>)),
							NodeInput::Network(concrete!(gpu_executor::ComputePassDimensions)),
						],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::CreateComputePassNode<_, _, _>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(gpu_executor::PipelineLayout<WgpuExecutor>)),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(ShaderInput<WgpuExecutor>)),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(gpu_executor::ComputePassDimensions)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "CommandBuffer",
				data_type: FrontendGraphDataType::General,
			}],
			properties: node_properties::input_properties,
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "CreatePipelineLayout",
			category: "Gpu",
			identifier: NodeImplementation::proto("gpu_executor::CreatePipelineLayoutNode<_, _, _, _>"),
			inputs: vec![
				DocumentInputType {
					name: "ShaderHandle",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(<WgpuExecutor as GpuExecutor>::ShaderHandle)),
				},
				DocumentInputType {
					name: "String",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(String)),
				},
				DocumentInputType {
					name: "Bindgroup",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(gpu_executor::Bindgroup<WgpuExecutor>)),
				},
				DocumentInputType {
					name: "ArcShaderInput",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(Arc<ShaderInput<WgpuExecutor>>)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "PipelineLayout",
				data_type: FrontendGraphDataType::General,
			}],
			properties: node_properties::input_properties,
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "ExecuteComputePipeline",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 0],
				outputs: vec![NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Execute Compute Pipeline".to_string(),
						inputs: vec![NodeInput::Network(concrete!(<WgpuExecutor as GpuExecutor>::CommandBuffer)), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::ExecuteComputePipelineNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "PipelineResult",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "ReadOutputBuffer",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 0],
				outputs: vec![NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Read Output Buffer".to_string(),
						inputs: vec![NodeInput::Network(concrete!(Arc<ShaderInput<WgpuExecutor>>)), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::ReadOutputBufferNode<_, _>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "Buffer",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "CreateGpuSurface",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![0],
				outputs: vec![NodeOutput::new(1, 0)],
				nodes: [
					DocumentNode {
						name: "Create Gpu Surface".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::CreateGpuSurfaceNode")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![DocumentInputType {
				name: "In",
				data_type: FrontendGraphDataType::General,
				default: NodeInput::Network(concrete!(WasmEditorApi)),
			}],
			outputs: vec![DocumentOutputType {
				name: "GpuSurface",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "RenderTexture",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 1, 0],
				outputs: vec![NodeOutput::new(1, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Render Texture".to_string(),
						inputs: vec![
							NodeInput::Network(concrete!(ShaderInputFrame<WgpuExecutor>)),
							NodeInput::Network(concrete!(Arc<SurfaceHandle<<WgpuExecutor as GpuExecutor>::Surface>>)),
							NodeInput::node(0, 0),
						],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::RenderTextureNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "Texture",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
				DocumentInputType {
					name: "Surface",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::None, true),
				},
				DocumentInputType {
					name: "EditorApi",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "RenderedTexture",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "UploadTexture",
			category: "Gpu",
			identifier: NodeImplementation::DocumentNode(NodeNetwork {
				inputs: vec![1, 0],
				outputs: vec![NodeOutput::new(2, 0)],
				nodes: [
					DocumentNode {
						name: "Extract Executor".to_string(),
						inputs: vec![NodeInput::Network(concrete!(WasmEditorApi))],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::ops::IntoNode<_, &WgpuExecutor>")),
						..Default::default()
					},
					DocumentNode {
						name: "Upload Texture".to_string(),
						inputs: vec![NodeInput::Network(concrete!(ImageFrame<Color>)), NodeInput::node(0, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("gpu_executor::UploadTextureNode<_>")),
						..Default::default()
					},
					DocumentNode {
						name: "Cache".to_string(),
						inputs: vec![NodeInput::ShortCircut(concrete!(())), NodeInput::node(1, 0)],
						implementation: DocumentNodeImplementation::Unresolved(NodeIdentifier::new("graphene_core::memo::MemoNode<_, _>")),
						..Default::default()
					},
				]
				.into_iter()
				.enumerate()
				.map(|(id, node)| (id as NodeId, node))
				.collect(),
				..Default::default()
			}),
			inputs: vec![
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType {
				name: "Texture",
				data_type: FrontendGraphDataType::General,
			}],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "GpuImage",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_std::executor::MapGpuSingleImageNode<_>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType {
					name: "Node",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::DocumentNode(DocumentNode::default()), true),
				},
				DocumentInputType {
					name: "In",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::Network(concrete!(WasmEditorApi)),
				},
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			..Default::default()
		},
		#[cfg(feature = "gpu")]
		DocumentNodeType {
			name: "Blend (GPU)",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_std::executor::BlendGpuImageNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Second", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Blend Mode", TaggedValue::BlendMode(BlendMode::Normal), false),
				DocumentInputType::value("Opacity", TaggedValue::F32(100.0), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::blend_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Extract",
			category: "Macros",
			identifier: NodeImplementation::Extract,
			inputs: vec![DocumentInputType {
				name: "Node",
				data_type: FrontendGraphDataType::General,
				default: NodeInput::value(TaggedValue::DocumentNode(DocumentNode::default()), true),
			}],
			outputs: vec![DocumentOutputType::new("DocumentNode", FrontendGraphDataType::General)],
			..Default::default()
		},
		#[cfg(feature = "quantization")]
		DocumentNodeType {
			name: "Generate Quantization",
			category: "Quantization",
			identifier: NodeImplementation::proto("graphene_std::quantization::GenerateQuantizationNode<_, _>"),
			inputs: vec![
				DocumentInputType {
					name: "Image",
					data_type: FrontendGraphDataType::Raster,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
				DocumentInputType {
					name: "samples",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::U32(100), false),
				},
				DocumentInputType {
					name: "Fn index",
					data_type: FrontendGraphDataType::Number,
					default: NodeInput::value(TaggedValue::U32(0), false),
				},
			],
			outputs: vec![DocumentOutputType::new("Quantization", FrontendGraphDataType::General)],
			properties: node_properties::quantize_properties,
			..Default::default()
		},
		#[cfg(feature = "quantization")]
		DocumentNodeType {
			name: "Quantize Image",
			category: "Quantization",
			identifier: NodeImplementation::proto("graphene_core::quantization::QuantizeNode<_>"),
			inputs: vec![
				DocumentInputType {
					name: "Image",
					data_type: FrontendGraphDataType::Raster,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
				DocumentInputType {
					name: "Quantization",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::Quantization(core::array::from_fn(|_| Default::default())), true),
				},
			],
			outputs: vec![DocumentOutputType::new("Encoded", FrontendGraphDataType::Raster)],
			properties: node_properties::quantize_properties,
			..Default::default()
		},
		#[cfg(feature = "quantization")]
		DocumentNodeType {
			name: "DeQuantize Image",
			category: "Quantization",
			identifier: NodeImplementation::proto("graphene_core::quantization::DeQuantizeNode<_>"),
			inputs: vec![
				DocumentInputType {
					name: "Encoded",
					data_type: FrontendGraphDataType::Raster,
					default: NodeInput::value(TaggedValue::ImageFrame(ImageFrame::empty()), true),
				},
				DocumentInputType {
					name: "Quantization",
					data_type: FrontendGraphDataType::General,
					default: NodeInput::value(TaggedValue::Quantization(core::array::from_fn(|_| Default::default())), true),
				},
			],
			outputs: vec![DocumentOutputType::new("Decoded", FrontendGraphDataType::Raster)],
			properties: node_properties::quantize_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Invert RGB",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::InvertRGBNode"),
			inputs: vec![DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true)],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Hue/Saturation",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::HueSaturationNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Hue Shift", TaggedValue::F32(0.), false),
				DocumentInputType::value("Saturation Shift", TaggedValue::F32(0.), false),
				DocumentInputType::value("Lightness Shift", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::adjust_hsl_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Brightness/Contrast",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::BrightnessContrastNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Brightness", TaggedValue::F32(0.), false),
				DocumentInputType::value("Contrast", TaggedValue::F32(0.), false),
				DocumentInputType::value("Use Legacy", TaggedValue::Bool(false), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::brightness_contrast_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Threshold",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::ThresholdNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Min Luminance", TaggedValue::F32(50.), false),
				DocumentInputType::value("Max Luminance", TaggedValue::F32(100.), false),
				DocumentInputType::value("Luminance Calc", TaggedValue::LuminanceCalculation(LuminanceCalculation::SRGB), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::adjust_threshold_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Vibrance",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::VibranceNode<_>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Vibrance", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::adjust_vibrance_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Channel Mixer",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::ChannelMixerNode<_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				// Monochrome toggle
				DocumentInputType::value("Monochrome", TaggedValue::Bool(false), false),
				// Monochrome
				DocumentInputType::value("Red", TaggedValue::F32(40.), false),
				DocumentInputType::value("Green", TaggedValue::F32(40.), false),
				DocumentInputType::value("Blue", TaggedValue::F32(20.), false),
				DocumentInputType::value("Constant", TaggedValue::F32(0.), false),
				// Red output channel
				DocumentInputType::value("(Red) Red", TaggedValue::F32(100.), false),
				DocumentInputType::value("(Red) Green", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Red) Blue", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Red) Constant", TaggedValue::F32(0.), false),
				// Green output channel
				DocumentInputType::value("(Green) Red", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Green) Green", TaggedValue::F32(100.), false),
				DocumentInputType::value("(Green) Blue", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Green) Constant", TaggedValue::F32(0.), false),
				// Blue output channel
				DocumentInputType::value("(Blue) Red", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blue) Green", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blue) Blue", TaggedValue::F32(100.), false),
				DocumentInputType::value("(Blue) Constant", TaggedValue::F32(0.), false),
				// Display-only properties (not used within the node)
				DocumentInputType::value("Output Channel", TaggedValue::RedGreenBlue(RedGreenBlue::Red), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::adjust_channel_mixer_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Selective Color",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto(
				"graphene_core::raster::SelectiveColorNode<_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _>",
			),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				// Mode
				DocumentInputType::value("Mode", TaggedValue::RelativeAbsolute(RelativeAbsolute::Relative), false),
				// Reds
				DocumentInputType::value("(Reds) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Reds) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Reds) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Reds) Black", TaggedValue::F32(0.), false),
				// Yellows
				DocumentInputType::value("(Yellows) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Yellows) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Yellows) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Yellows) Black", TaggedValue::F32(0.), false),
				// Greens
				DocumentInputType::value("(Greens) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Greens) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Greens) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Greens) Black", TaggedValue::F32(0.), false),
				// Cyans
				DocumentInputType::value("(Cyans) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Cyans) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Cyans) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Cyans) Black", TaggedValue::F32(0.), false),
				// Blues
				DocumentInputType::value("(Blues) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blues) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blues) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blues) Black", TaggedValue::F32(0.), false),
				// Magentas
				DocumentInputType::value("(Magentas) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Magentas) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Magentas) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Magentas) Black", TaggedValue::F32(0.), false),
				// Whites
				DocumentInputType::value("(Whites) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Whites) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Whites) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Whites) Black", TaggedValue::F32(0.), false),
				// Neutrals
				DocumentInputType::value("(Neutrals) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Neutrals) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Neutrals) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Neutrals) Black", TaggedValue::F32(0.), false),
				// Blacks
				DocumentInputType::value("(Blacks) Cyan", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blacks) Magenta", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blacks) Yellow", TaggedValue::F32(0.), false),
				DocumentInputType::value("(Blacks) Black", TaggedValue::F32(0.), false),
				// Display-only properties (not used within the node)
				DocumentInputType::value("Colors", TaggedValue::SelectiveColorChoice(SelectiveColorChoice::Reds), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::adjust_selective_color_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Opacity",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::OpacityNode<_>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Factor", TaggedValue::F32(100.), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::multiply_opacity,
			..Default::default()
		},
		DocumentNodeType {
			name: "Posterize",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::PosterizeNode<_>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Value", TaggedValue::F32(4.), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::posterize_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Exposure",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::ExposureNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Exposure", TaggedValue::F32(0.), false),
				DocumentInputType::value("Offset", TaggedValue::F32(0.), false),
				DocumentInputType::value("Gamma Correction", TaggedValue::F32(1.), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::exposure_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Add",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::AddParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("Primary", TaggedValue::F32(0.), true),
				DocumentInputType::value("Addend", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::add_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Subtract",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::AddParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("Primary", TaggedValue::F32(0.), true),
				DocumentInputType::value("Subtrahend", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::subtract_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Divide",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::DivideParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("Primary", TaggedValue::F32(0.), true),
				DocumentInputType::value("Divisor", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::divide_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Multiply",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::MultiplyParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("Primary", TaggedValue::F32(0.), true),
				DocumentInputType::value("Multiplicand", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::multiply_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Exponent",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::ExponentParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("Primary", TaggedValue::F32(0.), true),
				DocumentInputType::value("Power", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::exponent_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Max",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::MaxParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("First", TaggedValue::F32(0.), true),
				DocumentInputType::value("Second", TaggedValue::F32(0.), true),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::max_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Min",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::MinParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("First", TaggedValue::F32(0.), true),
				DocumentInputType::value("Second", TaggedValue::F32(0.), true),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::min_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Equality",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::EqParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("First", TaggedValue::F32(0.), true),
				DocumentInputType::value("Second", TaggedValue::F32(0.), true),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::eq_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Modulo",
			category: "Math",
			identifier: NodeImplementation::proto("graphene_core::ops::ModuloParameterNode<_>"),
			inputs: vec![
				DocumentInputType::value("Primary", TaggedValue::F32(0.), true),
				DocumentInputType::value("Modulus", TaggedValue::F32(0.), false),
			],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::Number)],
			properties: node_properties::modulo_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Log to Console",
			category: "Logic",
			identifier: NodeImplementation::proto("graphene_core::logic::LogToConsoleNode"),
			inputs: vec![DocumentInputType::value("First", TaggedValue::String("Not Connected to a value yet".into()), true)],
			outputs: vec![DocumentOutputType::new("Output", FrontendGraphDataType::General)],
			properties: node_properties::no_properties,
			..Default::default()
		},
		(*IMAGINATE_NODE).clone(),
		DocumentNodeType {
			name: "Unit Circle Generator",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::vector::generator_nodes::UnitCircleGenerator"),
			inputs: vec![DocumentInputType::none()],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Shape",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::vector::generator_nodes::PathGenerator<_>"),
			inputs: vec![
				DocumentInputType::value("Path Data", TaggedValue::Subpaths(vec![]), false),
				DocumentInputType::value("Mirror", TaggedValue::ManipulatorGroupIds(vec![]), false),
			],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Text",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::text::TextGenerator<_, _, _>"),
			inputs: vec![
				DocumentInputType::none(),
				DocumentInputType::value("Text", TaggedValue::String("hello world".to_string()), false),
				DocumentInputType::value("Font", TaggedValue::Font(Font::new(DEFAULT_FONT_FAMILY.into(), DEFAULT_FONT_STYLE.into())), false),
				DocumentInputType::value("Size", TaggedValue::F64(24.), false),
			],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			properties: node_properties::node_section_font,
			..Default::default()
		},
		DocumentNodeType {
			name: "Transform",
			category: "Transform",
			identifier: NodeImplementation::proto("graphene_core::transform::TransformNode<_, _, _, _, _>"),
			inputs: vec![
				DocumentInputType::value("Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true),
				DocumentInputType::value("Translation", TaggedValue::DVec2(DVec2::ZERO), false),
				DocumentInputType::value("Rotation", TaggedValue::F32(0.), false),
				DocumentInputType::value("Scale", TaggedValue::DVec2(DVec2::ONE), false),
				DocumentInputType::value("Skew", TaggedValue::DVec2(DVec2::ZERO), false),
				DocumentInputType::value("Pivot", TaggedValue::DVec2(DVec2::splat(0.5)), false),
			],
			outputs: vec![DocumentOutputType::new("Data", FrontendGraphDataType::Subpath)],
			properties: node_properties::transform_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "SetTransform",
			category: "Transform",
			identifier: NodeImplementation::proto("graphene_core::transform::SetTransformNode<_>"),
			inputs: vec![
				DocumentInputType::value("Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true),
				DocumentInputType::value("Transform", TaggedValue::DAffine2(DAffine2::IDENTITY), true),
			],
			outputs: vec![DocumentOutputType::new("Data", FrontendGraphDataType::Subpath)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Fill",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::vector::SetFillNode<_, _, _, _, _, _, _>"),
			inputs: vec![
				DocumentInputType::value("Vector Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true),
				DocumentInputType::value("Fill Type", TaggedValue::FillType(vector::style::FillType::None), false),
				DocumentInputType::value("Solid Color", TaggedValue::OptionalColor(None), false),
				DocumentInputType::value("Gradient Type", TaggedValue::GradientType(vector::style::GradientType::Linear), false),
				DocumentInputType::value("Start", TaggedValue::DVec2(DVec2::new(0., 0.5)), false),
				DocumentInputType::value("End", TaggedValue::DVec2(DVec2::new(1., 0.5)), false),
				DocumentInputType::value("Transform", TaggedValue::DAffine2(DAffine2::IDENTITY), false),
				DocumentInputType::value("Positions", TaggedValue::GradientPositions(vec![(0., Some(Color::BLACK)), (1., Some(Color::WHITE))]), false),
			],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			properties: node_properties::fill_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Stroke",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::vector::SetStrokeNode<_, _, _, _, _, _, _>"),
			inputs: vec![
				DocumentInputType::value("Vector Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true),
				DocumentInputType::value("Color", TaggedValue::OptionalColor(Some(Color::BLACK)), false),
				DocumentInputType::value("Weight", TaggedValue::F32(0.), false),
				DocumentInputType::value("Dash Lengths", TaggedValue::VecF32(Vec::new()), false),
				DocumentInputType::value("Dash Offset", TaggedValue::F32(0.), false),
				DocumentInputType::value("Line Cap", TaggedValue::LineCap(graphene_core::vector::style::LineCap::Butt), false),
				DocumentInputType::value("Line Join", TaggedValue::LineJoin(graphene_core::vector::style::LineJoin::Miter), false),
				DocumentInputType::value("Miter Limit", TaggedValue::F32(4.), false),
			],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			properties: node_properties::stroke_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Repeat",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::vector::RepeatNode<_, _>"),
			inputs: vec![
				DocumentInputType::value("Vector Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true),
				DocumentInputType::value("Direction", TaggedValue::DVec2((100., 0.).into()), false),
				DocumentInputType::value("Count", TaggedValue::U32(10), false),
			],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			properties: node_properties::repeat_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Bounding Box",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::vector::BoundingBoxNode"),
			inputs: vec![DocumentInputType::value("Vector Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true)],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			properties: node_properties::no_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Circular Repeat",
			category: "Vector",
			identifier: NodeImplementation::proto("graphene_core::vector::CircularRepeatNode<_, _, _>"),
			inputs: vec![
				DocumentInputType::value("Vector Data", TaggedValue::VectorData(graphene_core::vector::VectorData::empty()), true),
				DocumentInputType::value("Rotation Offset", TaggedValue::F32(0.), false),
				DocumentInputType::value("Radius", TaggedValue::F32(5.), false),
				DocumentInputType::value("Count", TaggedValue::U32(10), false),
			],
			outputs: vec![DocumentOutputType::new("Vector", FrontendGraphDataType::Subpath)],
			properties: node_properties::circle_repeat_properties,
			..Default::default()
		},
		DocumentNodeType {
			name: "Image Segmentation",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_std::image_segmentation::ImageSegmentationNode<_>"),
			inputs: vec![
				DocumentInputType::value("Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
				DocumentInputType::value("Mask", TaggedValue::ImageFrame(ImageFrame::empty()), true),
			],
			outputs: vec![DocumentOutputType::new("Segments", FrontendGraphDataType::Raster)],
			..Default::default()
		},
		DocumentNodeType {
			name: "Index",
			category: "Image Adjustments",
			identifier: NodeImplementation::proto("graphene_core::raster::IndexNode<_>"),
			inputs: vec![
				DocumentInputType::value("Segmentation", TaggedValue::Segments(vec![ImageFrame::empty()]), true),
				DocumentInputType::value("Index", TaggedValue::U32(0), false),
			],
			outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
			properties: node_properties::index_node_properties,
			..Default::default()
		},
	]
}

pub static IMAGINATE_NODE: Lazy<DocumentNodeType> = Lazy::new(|| DocumentNodeType {
	name: "Imaginate",
	category: "Image Synthesis",
	identifier: NodeImplementation::DocumentNode(NodeNetwork {
		inputs: vec![0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
		outputs: vec![NodeOutput::new(1, 0)],
		nodes: [
			(
				0,
				DocumentNode {
					name: "Frame Monitor".into(),
					inputs: vec![NodeInput::Network(concrete!(ImageFrame<Color>))],
					implementation: DocumentNodeImplementation::proto("graphene_core::memo::MonitorNode<_>"),
					..Default::default()
				},
			),
			(
				1,
				DocumentNode {
					name: "Imaginate".into(),
					inputs: vec![
						NodeInput::node(0, 0),
						NodeInput::Network(concrete!(WasmEditorApi)),
						NodeInput::Network(concrete!(ImaginateController)),
						NodeInput::Network(concrete!(f64)),
						NodeInput::Network(concrete!(Option<DVec2>)),
						NodeInput::Network(concrete!(u32)),
						NodeInput::Network(concrete!(ImaginateSamplingMethod)),
						NodeInput::Network(concrete!(f32)),
						NodeInput::Network(concrete!(String)),
						NodeInput::Network(concrete!(String)),
						NodeInput::Network(concrete!(bool)),
						NodeInput::Network(concrete!(f32)),
						NodeInput::Network(concrete!(Option<Vec<u64>>)),
						NodeInput::Network(concrete!(bool)),
						NodeInput::Network(concrete!(f32)),
						NodeInput::Network(concrete!(ImaginateMaskStartingFill)),
						NodeInput::Network(concrete!(bool)),
						NodeInput::Network(concrete!(bool)),
					],
					implementation: DocumentNodeImplementation::proto("graphene_std::raster::ImaginateNode<_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _>"),
					..Default::default()
				},
			),
		]
		.into(),
		..Default::default()
	}),
	inputs: vec![
		DocumentInputType::value("Input Image", TaggedValue::ImageFrame(ImageFrame::empty()), true),
		DocumentInputType {
			name: "Editor Api",
			data_type: FrontendGraphDataType::General,
			default: NodeInput::Network(concrete!(WasmEditorApi)),
		},
		DocumentInputType::value("Controller", TaggedValue::ImaginateController(Default::default()), false),
		DocumentInputType::value("Seed", TaggedValue::F64(0.), false), // Remember to keep index used in `ImaginateRandom` updated with this entry's index
		DocumentInputType::value("Resolution", TaggedValue::OptionalDVec2(None), false),
		DocumentInputType::value("Samples", TaggedValue::U32(30), false),
		DocumentInputType::value("Sampling Method", TaggedValue::ImaginateSamplingMethod(ImaginateSamplingMethod::EulerA), false),
		DocumentInputType::value("Prompt Guidance", TaggedValue::F32(7.5), false),
		DocumentInputType::value("Prompt", TaggedValue::String(String::new()), false),
		DocumentInputType::value("Negative Prompt", TaggedValue::String(String::new()), false),
		DocumentInputType::value("Adapt Input Image", TaggedValue::Bool(false), false),
		DocumentInputType::value("Image Creativity", TaggedValue::F32(66.), false),
		DocumentInputType::value("Masking Layer", TaggedValue::LayerPath(None), false),
		DocumentInputType::value("Inpaint", TaggedValue::Bool(true), false),
		DocumentInputType::value("Mask Blur", TaggedValue::F32(4.), false),
		DocumentInputType::value("Mask Starting Fill", TaggedValue::ImaginateMaskStartingFill(ImaginateMaskStartingFill::Fill), false),
		DocumentInputType::value("Improve Faces", TaggedValue::Bool(false), false),
		DocumentInputType::value("Tiling", TaggedValue::Bool(false), false),
	],
	outputs: vec![DocumentOutputType::new("Image", FrontendGraphDataType::Raster)],
	properties: node_properties::imaginate_properties,
	..Default::default()
});

pub fn resolve_document_node_type(name: &str) -> Option<&DocumentNodeType> {
	DOCUMENT_NODE_TYPES.iter().find(|node| node.name == name)
}

pub fn collect_node_types() -> Vec<FrontendNodeType> {
	DOCUMENT_NODE_TYPES
		.iter()
		.filter(|node_type| !node_type.category.eq_ignore_ascii_case("ignore"))
		.map(|node_type| FrontendNodeType::new(node_type.name, node_type.category))
		.collect()
}

impl DocumentNodeType {
	/// Generate a [`DocumentNodeImplementation`] from this node type, using a nested network.
	pub fn generate_implementation(&self) -> DocumentNodeImplementation {
		// let num_inputs = self.inputs.len();

		let inner_network = match &self.identifier {
			NodeImplementation::DocumentNode(network) => network.clone(),

			NodeImplementation::ProtoNode(ident) => return DocumentNodeImplementation::Unresolved(ident.clone()),
			/*
				NodeNetwork {
					inputs: (0..num_inputs).map(|_| 0).collect(),
					outputs: vec![NodeOutput::new(0, 0)],
					nodes: [(
						0,
						DocumentNode {
							name: format!("{}_impl", self.name),
							// TODO: Allow inserting nodes that contain other nodes.
							implementation: DocumentNodeImplementation::Unresolved(ident.clone()),
							inputs: self.inputs.iter().map(|i| NodeInput::Network(i.default.ty())).collect(),
							..Default::default()
						},
					)]
					.into_iter()
					.collect(),
					..Default::default()
				}

			}
			*/
			NodeImplementation::Extract => return DocumentNodeImplementation::Extract,
		};

		DocumentNodeImplementation::Network(inner_network)
	}

	/// Converts the [DocumentNodeType] type to a [DocumentNode], based on the inputs from the graph (which must be the correct length) and the metadata
	pub fn to_document_node(&self, inputs: impl IntoIterator<Item = NodeInput>, metadata: graph_craft::document::DocumentNodeMetadata) -> DocumentNode {
		let inputs: Vec<_> = inputs.into_iter().collect();
		assert_eq!(inputs.len(), self.inputs.len(), "Inputs passed from the graph must be equal to the number required");
		DocumentNode {
			name: self.name.to_string(),
			inputs,
			implementation: self.generate_implementation(),
			metadata,
			..Default::default()
		}
	}

	/// Converts the [DocumentNodeType] type to a [DocumentNode], using the provided `input_override` and falling back to the default inputs.
	/// `input_override` does not have to be the correct length.
	pub fn to_document_node_default_inputs(&self, input_override: impl IntoIterator<Item = Option<NodeInput>>, metadata: graph_craft::document::DocumentNodeMetadata) -> DocumentNode {
		let mut input_override = input_override.into_iter();
		let inputs = self.inputs.iter().map(|default| input_override.next().unwrap_or_default().unwrap_or_else(|| default.default.clone()));
		self.to_document_node(inputs, metadata)
	}
}

pub fn wrap_network_in_scope(mut network: NodeNetwork) -> NodeNetwork {
	network.generate_node_paths(&[]);

	let node_ids = network.nodes.keys().copied().collect::<Vec<_>>();
	for id in node_ids {
		network.flatten(id);
	}

	let mut network_inputs = Vec::new();
	let mut input_type = None;
	for (id, node) in network.nodes.iter() {
		for input in node.inputs.iter() {
			if let NodeInput::Network(_) = input {
				if input_type.is_none() {
					input_type = Some(input.clone());
				}
				assert_eq!(input, input_type.as_ref().unwrap(), "Networks wrapped in scope must have the same input type");
				network_inputs.push(*id);
			}
		}
	}
	let len = network_inputs.len();
	network.inputs = network_inputs;

	// if the network has no inputs, it doesn't need to be wrapped in a scope
	if len == 0 {
		return network;
	}

	let inner_network = DocumentNode {
		name: "Scope".to_string(),
		implementation: DocumentNodeImplementation::Network(network),
		inputs: core::iter::repeat(NodeInput::node(0, 1)).take(len).collect(),
		..Default::default()
	};

	// wrap the inner network in a scope
	let nodes = vec![
		resolve_document_node_type("Begin Scope")
			.expect("Begin Scope node type not found")
			.to_document_node(vec![input_type.unwrap()], DocumentNodeMetadata::default()),
		inner_network,
		resolve_document_node_type("End Scope")
			.expect("End Scope node type not found")
			.to_document_node(vec![NodeInput::node(0, 0), NodeInput::node(1, 0)], DocumentNodeMetadata::default()),
	];

	NodeNetwork {
		inputs: vec![0],
		outputs: vec![NodeOutput::new(2, 0)],
		nodes: nodes.into_iter().enumerate().map(|(id, node)| (id as NodeId, node)).collect(),
		..Default::default()
	}
}

pub fn new_image_network(output_offset: i32, output_node_id: NodeId) -> NodeNetwork {
	let mut network = NodeNetwork {
		inputs: vec![0],
		..Default::default()
	};
	network.push_node(
		resolve_document_node_type("Input Frame")
			.expect("Input Frame node does not exist")
			.to_document_node_default_inputs([], DocumentNodeMetadata::position((8, 4))),
		false,
	);
	network.push_node(
		resolve_document_node_type("Output")
			.expect("Output node does not exist")
			.to_document_node([NodeInput::node(output_node_id, 0)], DocumentNodeMetadata::position((output_offset + 8, 4))),
		false,
	);
	network
}

pub fn new_vector_network(subpaths: Vec<bezier_rs::Subpath<uuid::ManipulatorGroupId>>) -> NodeNetwork {
	let path_generator = resolve_document_node_type("Shape").expect("Shape node does not exist");
	let transform = resolve_document_node_type("Transform").expect("Transform node does not exist");
	let fill = resolve_document_node_type("Fill").expect("Fill node does not exist");
	let stroke = resolve_document_node_type("Stroke").expect("Stroke node does not exist");
	let output = resolve_document_node_type("Output").expect("Output node does not exist");

	let mut network = NodeNetwork {
		inputs: vec![0],
		..Default::default()
	};

	network.push_node(
		path_generator.to_document_node_default_inputs([Some(NodeInput::value(TaggedValue::Subpaths(subpaths), false))], DocumentNodeMetadata::position((0, 4))),
		false,
	);
	network.push_node(transform.to_document_node_default_inputs([None], Default::default()), true);
	network.push_node(fill.to_document_node_default_inputs([None], Default::default()), true);
	network.push_node(stroke.to_document_node_default_inputs([None], Default::default()), true);
	network.push_node(output.to_document_node_default_inputs([None], Default::default()), true);
	network
}

pub fn new_text_network(text: String, font: Font, size: f64) -> NodeNetwork {
	let text_generator = resolve_document_node_type("Text").expect("Text node does not exist");
	let transform = resolve_document_node_type("Transform").expect("Transform node does not exist");
	let fill = resolve_document_node_type("Fill").expect("Fill node does not exist");
	let stroke = resolve_document_node_type("Stroke").expect("Stroke node does not exist");
	let output = resolve_document_node_type("Output").expect("Output node does not exist");

	let mut network = NodeNetwork {
		inputs: vec![0],
		..Default::default()
	};
	network.push_node(
		text_generator.to_document_node(
			[
				NodeInput::Network(concrete!(WasmEditorApi)),
				NodeInput::value(TaggedValue::String(text), false),
				NodeInput::value(TaggedValue::Font(font), false),
				NodeInput::value(TaggedValue::F64(size), false),
			],
			DocumentNodeMetadata::position((0, 4)),
		),
		false,
	);
	network.push_node(transform.to_document_node_default_inputs([None], Default::default()), true);
	network.push_node(fill.to_document_node_default_inputs([None], Default::default()), true);
	network.push_node(stroke.to_document_node_default_inputs([None], Default::default()), true);
	network.push_node(output.to_document_node_default_inputs([None], Default::default()), true);
	network
}
