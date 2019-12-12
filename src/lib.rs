//! The Web Canvas backend for the Piet 2D graphics abstraction.

use std::borrow::Cow;
use std::ops::Deref;

use piet::kurbo::{Affine, PathEl, Point, Rect, Shape};

use stdweb::web::{CanvasRenderingContext2d, CanvasGradient};
use stdweb::js;

pub use piet::*;

pub struct WebRenderContext<'a> {
    ctx: &'a mut CanvasRenderingContext2d,
    /// Used for creating image bitmaps and possibly other resources.
//     window: &'a Window,
    err: Result<(), Error>,
}

impl<'a> WebRenderContext<'a> {
    pub fn new(ctx: &'a mut CanvasRenderingContext2d/*, window: &'a Window*/) -> WebRenderContext<'a> {
        WebRenderContext {
            ctx,
//             window,
            err: Ok(()),
        }
    }
}

#[derive(Clone)]
pub enum Brush {
    Solid(u32),
    Gradient(CanvasGradient),
}

#[derive(Clone)]
pub struct WebFont {
    family: String,
    weight: u32,
    style: FontStyle,
    size: f64,
}

pub struct WebFontBuilder(WebFont);

pub struct WebTextLayout {
    font: WebFont,
    text: String,
    width: f64,
}

pub struct WebTextLayoutBuilder {
    ctx: CanvasRenderingContext2d,
    font: WebFont,
    text: String,
}

pub struct WebImage {
    inner: stdweb::Value,
//     inner: Vec<u8>,
    width: u32,
    height: u32,
//     format: ImageFormat,
}

/// https://developer.mozilla.org/en-US/docs/Web/CSS/font-style
#[allow(dead_code)] // TODO: Remove
#[derive(Clone)]
enum FontStyle {
    Normal,
    Italic,
    Oblique(Option<f64>),
}

trait WrapError<T> {
    fn wrap(self) -> Result<T, Error>;
}

impl<T, E : std::error::Error + 'static> WrapError<T> for Result<T, E> {
    fn wrap(self) -> Result<T, Error> {
        self.map_err(|e| {
            let e: Box<dyn std::error::Error> = Box::new(e);
            e.into()
        })
    }
}

fn convert_line_cap(line_cap: LineCap) -> stdweb::web::LineCap {
    match line_cap {
        LineCap::Butt => stdweb::web::LineCap::Butt,
        LineCap::Round => stdweb::web::LineCap::Round,
        LineCap::Square => stdweb::web::LineCap::Square,
    }
}

fn convert_line_join(line_join: LineJoin) -> stdweb::web::LineJoin {
    match line_join {
        LineJoin::Miter => stdweb::web::LineJoin::Miter,
        LineJoin::Round => stdweb::web::LineJoin::Round,
        LineJoin::Bevel => stdweb::web::LineJoin::Bevel,
    }
}

impl<'a> RenderContext for WebRenderContext<'a> {
    /// wasm-bindgen doesn't have a native Point type, so use kurbo's.
    type Brush = Brush;

    type Text = Self;
    type TextLayout = WebTextLayout;

    type Image = WebImage;

    fn status(&mut self) -> Result<(), Error> {
        std::mem::replace(&mut self.err, Ok(()))
    }

    fn clear(&mut self, color: Color) {
        let canvas = self.ctx.get_canvas();
        let shape = Rect::new(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);
        let brush = self.solid_brush(color);
        self.fill(shape, &brush);
    }

    fn solid_brush(&mut self, color: Color) -> Brush {
        Brush::Solid(color.as_rgba_u32())
    }

    fn gradient(&mut self, gradient: impl Into<FixedGradient>) -> Result<Brush, Error> {
        match gradient.into() {
            FixedGradient::Linear(linear) => {
                let (x0, y0) = (linear.start.x, linear.start.y);
                let (x1, y1) = (linear.end.x, linear.end.y);
                let mut lg = self.ctx.create_linear_gradient(x0, y0, x1, y1);
                set_gradient_stops(&mut lg, &linear.stops);
                Ok(Brush::Gradient(lg))
            }
            FixedGradient::Radial(radial) => {
                let (xc, yc) = (radial.center.x, radial.center.y);
                let (xo, yo) = (radial.origin_offset.x, radial.origin_offset.y);
                let r = radial.radius;
                let mut rg = self
                    .ctx
                    .create_radial_gradient(xc + xo, yc + yo, 0.0, xc, yc, r).wrap()?;
                set_gradient_stops(&mut rg, &radial.stops);
                Ok(Brush::Gradient(rg))
            }
        }
    }

    fn fill(&mut self, shape: impl Shape, brush: &impl IntoBrush<Self>) {
        let brush = brush.make_brush(self, || shape.bounding_box());
        self.set_path(shape);
        self.set_brush(&*brush, true);
        self.ctx.fill(stdweb::web::FillRule::NonZero);
    }

    fn fill_even_odd(&mut self, shape: impl Shape, brush: &impl IntoBrush<Self>) {
        let brush = brush.make_brush(self, || shape.bounding_box());
        self.set_path(shape);
        self.set_brush(&*brush, true);
        self.ctx.fill(stdweb::web::FillRule::EvenOdd);
    }

    fn clip(&mut self, shape: impl Shape) {
        self.set_path(shape);
        self.ctx.fill(stdweb::web::FillRule::NonZero);
    }

    fn stroke(&mut self, shape: impl Shape, brush: &impl IntoBrush<Self>, width: f64) {
        let brush = brush.make_brush(self, || shape.bounding_box());
        self.set_path(shape);
        self.set_stroke(width, None);
        self.set_brush(&*brush.deref(), false);
        self.ctx.stroke();
    }

    fn stroke_styled(
        &mut self,
        shape: impl Shape,
        brush: &impl IntoBrush<Self>,
        width: f64,
        style: &StrokeStyle,
    ) {
        let brush = brush.make_brush(self, || shape.bounding_box());
        self.set_path(shape);
        self.set_stroke(width, Some(style));
        self.set_brush(&*brush.deref(), false);
        self.ctx.stroke();
    }

    fn text(&mut self) -> &mut Self::Text {
        self
    }

    fn draw_text(
        &mut self,
        layout: &Self::TextLayout,
        pos: impl Into<Point>,
        brush: &impl IntoBrush<Self>,
    ) {
        // TODO: bounding box for text
        let brush = brush.make_brush(self, || Rect::ZERO);
        self.ctx.set_font(&layout.font.get_font_string());
        self.set_brush(&*brush, true);
        let pos = pos.into();
        self.ctx.fill_text(&layout.text, pos.x, pos.y, None);
    }

    fn save(&mut self) -> Result<(), Error> {
        self.ctx.save();
        Ok(())
    }

    fn restore(&mut self) -> Result<(), Error> {
        self.ctx.restore();
        Ok(())
    }

    fn finish(&mut self) -> Result<(), Error> {
        self.status()
    }

    fn transform(&mut self, transform: Affine) {
        let a = transform.as_coeffs();
        let _ = self.ctx.transform(a[0], a[1], a[2], a[3], a[4], a[5]);
    }

    fn make_image(
        &mut self,
        width: usize,
        height: usize,
        buf: &[u8],
        format: ImageFormat,
    ) -> Result<Self::Image, Error> {

        /*let buf = match format {
            // Discussion topic: if buf were mut here, we could probably avoid this clone.
            // See https://github.com/rustwasm/wasm-bindgen/issues/1005 for an issue that might
            // also resolve the need to clone.
            ImageFormat::RgbaSeparate => buf.to_vec(),
            ImageFormat::RgbaPremul => {
                fn unpremul(x: u8, a: u8) -> u8 {
                    if a == 0 {
                        0
                    } else {
                        let y = (x as u32 * 255 + (a as u32 / 2)) / (a as u32);
                        y.min(255) as u8
                    }
                }
                let mut new_buf = vec![0; width * height * 4];
                for i in 0..width * height {
                    let a = buf[i * 4 + 3];
                    new_buf[i * 4 + 0] = unpremul(buf[i * 4 + 0], a);
                    new_buf[i * 4 + 1] = unpremul(buf[i * 4 + 1], a);
                    new_buf[i * 4 + 2] = unpremul(buf[i * 4 + 2], a);
                    new_buf[i * 4 + 3] = a;
                }
                new_buf
            }
            ImageFormat::Rgb => {
                let mut new_buf = vec![0; width * height * 4];
                for i in 0..width * height {
                    new_buf[i * 4 + 0] = buf[i * 3 + 0];
                    new_buf[i * 4 + 1] = buf[i * 3 + 1];
                    new_buf[i * 4 + 2] = buf[i * 3 + 2];
                    new_buf[i * 4 + 3] = 255;
                }
                new_buf
            }
            _ => Vec::new(),
        };*/

        let image_data = js!{
            var data = new ImageData(new Uint8ClampedArray(@{buf}), @{width as u32}, @{height as u32});
            var canvas = document.createElement("canvas");
            canvas.width = @{width as u32};
            canvas.height = @{height as u32};
            var ctx = canvas.getContext("2d");
            ctx.putImageData(data, 0, 0);
            return canvas;
        };
        Ok(WebImage {
            inner: image_data,
            width: width as u32,
            height: height as u32,
        })
    }

    fn draw_image(
        &mut self,
        image: &Self::Image,
        rect: impl Into<Rect>,
        _interp: InterpolationMode,
    ) {
        self.with_save(|rc| {
            let rect = rect.into();
            js!{
                var ctx = @{rc.ctx.clone()};
                var inner = @{image.inner.clone()};
                ctx.drawImage(inner,
                   // 0, 0, @{image.width}, @{image.height},
                    @{rect.x0}, @{rect.y0},
                    @{rect.width()},
                    @{rect.height()});
            };
            Ok(())
        }).unwrap();
    }
}

impl<'a> IntoBrush<WebRenderContext<'a>> for Brush {
    fn make_brush<'b>(
        &'b self,
        _piet: &mut WebRenderContext,
        _bbox: impl FnOnce() -> Rect,
    ) -> std::borrow::Cow<'b, Brush> {
        Cow::Borrowed(self)
    }
}

fn format_color(rgba: u32) -> String {
    let rgb = rgba >> 8;
    let a = rgba & 0xff;
    if a == 0xff {
        format!("#{:06x}", rgba >> 8)
    } else {
        format!(
            "rgba({},{},{},{:.3})",
            (rgb >> 16) & 0xff,
            (rgb >> 8) & 0xff,
            rgb & 0xff,
            byte_to_frac(a)
        )
    }
}

fn set_gradient_stops(dst: &mut CanvasGradient, src: &[GradientStop]) {
    for stop in src {
        // TODO: maybe get error?
        let rgba = stop.color.as_rgba_u32();
        let _ = dst.add_color_stop(stop.pos as f64, &format_color(rgba));
    }
}

impl<'a> Text for WebRenderContext<'a> {
    type Font = WebFont;
    type FontBuilder = WebFontBuilder;
    type TextLayout = WebTextLayout;
    type TextLayoutBuilder = WebTextLayoutBuilder;

    fn new_font_by_name(&mut self, name: &str, size: f64) -> Result<Self::FontBuilder, Error> {
        let font = WebFont {
            family: name.to_owned(),
            size,
            weight: 400,
            style: FontStyle::Normal,
        };
        Ok(WebFontBuilder(font))
    }

    fn new_text_layout(&mut self, font: &Self::Font, text: &str) -> Result<Self::TextLayoutBuilder, Error> {
        Ok(WebTextLayoutBuilder {
            // TODO: it's very likely possible to do this without cloning ctx, but
            // I couldn't figure out the lifetime errors from a `&'a` reference.
            ctx: self.ctx.clone(),
            font: font.clone(),
            text: text.to_owned(),
        })
    }
}

impl<'a> WebRenderContext<'a> {
    /// Set the source pattern to the brush.
    ///
    /// Web canvas is super stateful, and we're trying to have more retained stuff.
    /// This is part of the impedance matching.
    fn set_brush(&mut self, brush: &Brush, is_fill: bool) {
        match *brush {
            Brush::Solid(rgba) => {
                let color_str = format_color(rgba);
                if is_fill {
                    self.ctx.set_fill_style_color(&color_str);
                } else {
                    self.ctx.set_stroke_style_color(&color_str);
                }
            }
            Brush::Gradient(ref gradient) => {
                if is_fill {
                    self.ctx.set_fill_style_gradient(gradient);
                } else {
                    self.ctx.set_stroke_style_gradient(gradient);
                }
            }
        }
    }

    /// Set the stroke parameters.
    ///
    /// TODO(performance): this is probably expensive enough it makes sense
    /// to at least store the last version and only reset if it's changed.
    fn set_stroke(&mut self, width: f64, style: Option<&StrokeStyle>) {
        //TODO
        /*self.ctx.set_line_width(width);

        let line_join = style
            .and_then(|style| style.line_join)
            .unwrap_or(LineJoin::Miter);
        self.ctx.set_line_join(convert_line_join(line_join));

        let line_cap = style
            .and_then(|style| style.line_cap)
            .unwrap_or(LineCap::Butt);
        self.ctx.set_line_cap(convert_line_cap(line_cap));

        let miter_limit = style.and_then(|style| style.miter_limit).unwrap_or(10.0);
        self.ctx.set_miter_limit(miter_limit);

        let (dash_segs, dash_offset) = style
            .and_then(|style| style.dash.as_ref())
            .map(|dash| {
                let len = dash.0.len() as u32;
                let array = Float64Array::new_with_length(len);
                for (i, elem) in dash.0.iter().enumerate() {
                    Reflect::set(
                        array.as_ref(),
                        &JsValue::from(i as u32),
                        &JsValue::from(*elem),
                    )
                    .unwrap();
                }
                (array, dash.1)
            })
            .unwrap_or((Float64Array::new_with_length(0), 0.0));

        self.ctx.set_line_dash(dash_segs.as_ref()).unwrap();
        self.ctx.set_line_dash_offset(dash_offset);*/
    }

    fn set_path(&mut self, shape: impl Shape) {
        // This shouldn't be necessary, we always leave the context in no-path
        // state. But just in case, and it should be harmless.
        self.ctx.begin_path();
        for el in shape.to_bez_path(1e-3) {
            match el {
                PathEl::MoveTo(p) => self.ctx.move_to(p.x, p.y),
                PathEl::LineTo(p) => self.ctx.line_to(p.x, p.y),
                PathEl::QuadTo(p1, p2) => self.ctx.quadratic_curve_to(p1.x, p1.y, p2.x, p2.y),
                PathEl::CurveTo(p1, p2, p3) => {
                    self.ctx.bezier_curve_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y)
                }
                PathEl::ClosePath => self.ctx.close_path(),
            }
        }
    }
}

fn byte_to_frac(byte: u32) -> f64 {
    ((byte & 255) as f64) * (1.0 / 255.0)
}

impl FontBuilder for WebFontBuilder {
    type Out = WebFont;

    fn build(self) -> Result<Self::Out, Error> {
        Ok(self.0)
    }
}

impl Font for WebFont {}

impl WebFont {
    fn get_font_string(&self) -> String {
        let style_str = match self.style {
            FontStyle::Normal => Cow::from("normal"),
            FontStyle::Italic => Cow::from("italic"),
            FontStyle::Oblique(None) => Cow::from("italic"),
            FontStyle::Oblique(Some(angle)) => Cow::from(format!("oblique {}deg", angle)),
        };
        format!(
            "{} {} {}px \"{}\"",
            style_str, self.weight, self.size, self.family
        )
    }
}

impl TextLayoutBuilder for WebTextLayoutBuilder {
    type Out = WebTextLayout;

    fn build(self) -> Result<Self::Out, Error> {
        self.ctx.set_font(&self.font.get_font_string());
        let width = self
            .ctx
            .measure_text(&self.text).wrap()?
            .get_width();
        Ok(WebTextLayout {
            font: self.font,
            text: self.text,
            width,
        })
    }
}

impl TextLayout for WebTextLayout {
    fn width(&self) -> f64 {
        self.width
    }
}


pub type Piet<'a> = WebRenderContext<'a>;
pub type PietText<'a> = WebRenderContext<'a>;
pub type PietFont = WebFont;
pub type PietFontBuilder<'a> = WebFontBuilder;
pub type PietTextLayout = WebTextLayout;
pub type PietTextLayoutBuilder<'a> = WebTextLayoutBuilder;
pub type Image = WebImage;
