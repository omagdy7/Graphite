use crate::raster::{Image, ImageFrame};
use crate::{uuid::generate_uuid, vector::VectorData, Artboard, Color, GraphicElementData, GraphicGroup};
use quad::Quad;

use glam::{DAffine2, DVec2};

mod quad;

/// Mutable state used whilst rendering to an SVG
pub struct SvgRender {
	pub svg: SvgSegmentList,
	pub svg_defs: String,
	pub transform: DAffine2,
	pub image_data: Vec<(u64, Image<Color>)>,
	indent: usize,
}

impl SvgRender {
	pub fn new() -> Self {
		Self {
			svg: SvgSegmentList::default(),
			svg_defs: String::new(),
			transform: DAffine2::IDENTITY,
			image_data: Vec::new(),
			indent: 0,
		}
	}

	pub fn indent(&mut self) {
		self.svg.push("\n");
		self.svg.push("\t".repeat(self.indent));
	}

	/// Add an outer `<svg />` tag with a `viewBox` and the `<defs />`
	pub fn format_svg(&mut self, bounds_min: DVec2, bounds_max: DVec2) {
		let (x, y) = bounds_min.into();
		let (size_x, size_y) = (bounds_max - bounds_min).into();
		let defs = &self.svg_defs;
		let svg_header = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x} {y} {size_x} {size_y}"><defs>{defs}</defs>"#,);
		self.svg.insert(0, svg_header.into());
		self.svg.push("</svg>");
	}

	pub fn leaf_tag(&mut self, name: impl Into<SvgSegment>, attributes: impl FnOnce(&mut SvgRenderAttrs)) {
		self.indent();
		self.svg.push("<");
		self.svg.push(name);
		attributes(&mut SvgRenderAttrs(self));

		self.svg.push("/>");
	}

	pub fn parent_tag(&mut self, name: impl Into<SvgSegment>, attributes: impl FnOnce(&mut SvgRenderAttrs), inner: impl FnOnce(&mut Self)) {
		let name = name.into();
		self.indent();
		self.svg.push("<");
		self.svg.push(name.clone());
		attributes(&mut SvgRenderAttrs(self));
		self.svg.push(">");
		let length = self.svg.len();
		self.indent += 1;
		inner(self);
		self.indent -= 1;
		if self.svg.len() != length {
			self.indent();
			self.svg.push("</");
			self.svg.push(name);
			self.svg.push(">");
		} else {
			self.svg.pop();
			self.svg.push("/>");
		}
	}
}

impl Default for SvgRender {
	fn default() -> Self {
		Self::new()
	}
}

/// Static state used whilst rendering
pub struct RenderParams {
	pub view_mode: crate::vector::style::ViewMode,
	pub culling_bounds: Option<[DVec2; 2]>,
	pub thumbnail: bool,
}

impl RenderParams {
	pub fn new(view_mode: crate::vector::style::ViewMode, culling_bounds: Option<[DVec2; 2]>, thumbnail: bool) -> Self {
		Self { view_mode, culling_bounds, thumbnail }
	}
}

pub fn format_transform_matrix(transform: DAffine2) -> String {
	use std::fmt::Write;
	let mut result = "matrix(".to_string();
	let cols = transform.to_cols_array();
	for (index, item) in cols.iter().enumerate() {
		write!(result, "{}", item).unwrap();
		if index != cols.len() - 1 {
			result.push_str(", ");
		}
	}
	result.push(')');
	result
}

pub trait GraphicElementRendered {
	fn render_svg(&self, render: &mut SvgRender, render_params: &RenderParams);
	fn bounding_box(&self, transform: DAffine2) -> Option<[DVec2; 2]>;
}

impl GraphicElementRendered for GraphicGroup {
	fn render_svg(&self, render: &mut SvgRender, render_params: &RenderParams) {
		self.iter().for_each(|element| element.graphic_element_data.render_svg(render, render_params))
	}
	fn bounding_box(&self, transform: DAffine2) -> Option<[DVec2; 2]> {
		self.iter().filter_map(|element| element.graphic_element_data.bounding_box(transform)).reduce(Quad::combine_bounds)
	}
}

impl GraphicElementRendered for VectorData {
	fn render_svg(&self, render: &mut SvgRender, render_params: &RenderParams) {
		let layer_bounds = self.bounding_box().unwrap_or_default();
		let transformed_bounds = self.bounding_box_with_transform(render.transform).unwrap_or_default();

		let mut path = String::new();
		for subpath in &self.subpaths {
			let _ = subpath.subpath_to_svg(&mut path, self.transform * render.transform);
		}
		render.leaf_tag("path", |attributes| {
			attributes.push("class", "vector-data");
			attributes.push("d", path);
			let render = &mut attributes.0;
			let style = self.style.render(render_params.view_mode, &mut render.svg_defs, render.transform, layer_bounds, transformed_bounds);
			attributes.push_val(style);
		});
	}
	fn bounding_box(&self, transform: DAffine2) -> Option<[DVec2; 2]> {
		self.bounding_box_with_transform(self.transform * transform)
	}
}

impl GraphicElementRendered for Artboard {
	fn render_svg(&self, render: &mut SvgRender, render_params: &RenderParams) {
		// Background
		render.leaf_tag("rect", |attributes| {
			attributes.push("class", "artboard-bg");
			attributes.push("fill", format!("#{}", self.background.rgba_hex()));
			attributes.push("x", self.location.x.min(self.location.x + self.dimensions.x).to_string());
			attributes.push("y", self.location.y.min(self.location.y + self.dimensions.y).to_string());
			attributes.push("width", self.dimensions.x.abs().to_string());
			attributes.push("height", self.dimensions.y.abs().to_string());
		});

		// Label
		render.parent_tag(
			"text",
			|attributes| {
				attributes.push("class", "artboard-label");
				attributes.push("fill", "white");
				attributes.push("x", (self.location.x.min(self.location.x + self.dimensions.x)).to_string());
				attributes.push("y", (self.location.y.min(self.location.y + self.dimensions.y) - 4).to_string());
				attributes.push("font-size", "14px");
			},
			|render| {
				render.svg.push("Artboard");
			},
		);

		// Contents group
		render.parent_tag(
			"g",
			|attributes| {
				attributes.push("class", "artboard");
				if self.clip {
					let id = format!("artboard-{}", generate_uuid());
					let selector = format!("url(#{id})");
					use std::fmt::Write;
					write!(
						&mut attributes.0.svg_defs,
						r##"<clipPath id="{id}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath>"##,
						self.location.x, self.location.y, self.dimensions.x, self.dimensions.y
					)
					.unwrap();
					attributes.push("clip-path", selector);
				}
			},
			|render| {
				// Contents
				self.graphic_group.render_svg(render, render_params);
			},
		);
	}
	fn bounding_box(&self, transform: DAffine2) -> Option<[DVec2; 2]> {
		let artboard_bounds = (transform * Quad::from_box([self.location.as_dvec2(), self.location.as_dvec2() + self.dimensions.as_dvec2()])).bounding_box();
		[self.graphic_group.bounding_box(transform), Some(artboard_bounds)].into_iter().flatten().reduce(Quad::combine_bounds)
	}
}

impl GraphicElementRendered for ImageFrame<Color> {
	fn render_svg(&self, render: &mut SvgRender, _render_params: &RenderParams) {
		let transform: String = format_transform_matrix(self.transform * render.transform);
		let uuid = generate_uuid();
		render.leaf_tag("image", |attributes| {
			attributes.push("width", 1.to_string());
			attributes.push("height", 1.to_string());
			attributes.push("preserveAspectRatio", "none");
			attributes.push("transform", transform);
			attributes.push("href", SvgSegment::BlobUrl(uuid))
		});
		render.image_data.push((uuid, self.image.clone()))
	}
	fn bounding_box(&self, transform: DAffine2) -> Option<[DVec2; 2]> {
		let transform = self.transform * transform;
		(transform.matrix2 != glam::DMat2::ZERO).then(|| (transform * Quad::from_box([DVec2::ZERO, DVec2::ONE])).bounding_box())
	}
}

impl GraphicElementRendered for GraphicElementData {
	fn render_svg(&self, render: &mut SvgRender, render_params: &RenderParams) {
		match self {
			GraphicElementData::VectorShape(vector_data) => vector_data.render_svg(render, render_params),
			GraphicElementData::ImageFrame(image_frame) => image_frame.render_svg(render, render_params),
			GraphicElementData::Text(_) => todo!("Render a text GraphicElementData"),
			GraphicElementData::GraphicGroup(graphic_group) => graphic_group.render_svg(render, render_params),
			GraphicElementData::Artboard(artboard) => artboard.render_svg(render, render_params),
		}
	}

	fn bounding_box(&self, transform: DAffine2) -> Option<[DVec2; 2]> {
		match self {
			GraphicElementData::VectorShape(vector_data) => GraphicElementRendered::bounding_box(&**vector_data, transform),
			GraphicElementData::ImageFrame(image_frame) => image_frame.bounding_box(transform),
			GraphicElementData::Text(_) => todo!("Bounds of a text GraphicElementData"),
			GraphicElementData::GraphicGroup(graphic_group) => graphic_group.bounding_box(transform),
			GraphicElementData::Artboard(artboard) => artboard.bounding_box(transform),
		}
	}
}

/// A segment of an svg string to allow for embedding blob urls
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvgSegment {
	Slice(&'static str),
	String(String),
	BlobUrl(u64),
}

impl From<String> for SvgSegment {
	fn from(value: String) -> Self {
		Self::String(value)
	}
}

impl From<&'static str> for SvgSegment {
	fn from(value: &'static str) -> Self {
		Self::Slice(value)
	}
}

/// A list of [`SvgSegment`]s.
///
/// Can be modified with `list.push("hello".into())`. Use `list.to_string()` to convert the segments into one string.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SvgSegmentList(Vec<SvgSegment>);

impl core::ops::Deref for SvgSegmentList {
	type Target = Vec<SvgSegment>;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}
impl core::ops::DerefMut for SvgSegmentList {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl core::fmt::Display for SvgSegmentList {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		for segment in self.iter() {
			f.write_str(match segment {
				SvgSegment::Slice(x) => x,
				SvgSegment::String(x) => x,
				SvgSegment::BlobUrl(_) => "<!-- Blob url not yet loaded -->",
			})?;
		}
		Ok(())
	}
}

pub struct SvgRenderAttrs<'a>(&'a mut SvgRender);

impl<'a> SvgRenderAttrs<'a> {
	pub fn push_complex(&mut self, name: impl Into<SvgSegment>, value: impl FnOnce(&mut SvgRender)) {
		self.0.svg.push(" ");
		self.0.svg.push(name);
		self.0.svg.push("=\"");
		value(self.0);
		self.0.svg.push("\"");
	}
	pub fn push(&mut self, name: impl Into<SvgSegment>, value: impl Into<SvgSegment>) {
		self.push_complex(name, move |renderer| renderer.svg.push(value));
	}
	pub fn push_val(&mut self, value: impl Into<SvgSegment>) {
		self.0.svg.push(value);
	}
}

impl SvgSegmentList {
	pub fn push(&mut self, value: impl Into<SvgSegment>) {
		self.0.push(value.into());
	}
}
