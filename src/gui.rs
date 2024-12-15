use embedded_graphics::{
    mono_font::MonoTextStyle,
    pixelcolor::BinaryColor,
    prelude::{Angle, DrawTarget, Point, Primitive, Size, Transform},
    primitives::{
        Arc, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, StrokeAlignment,
        StyledDrawable, Triangle,
    },
    text::Text,
    Drawable,
};
use ssd1306::{
    mode::BufferedGraphicsMode, prelude::WriteOnlyDataCommand, size::DisplaySize, Ssd1306,
};

use crate::{AppState, UIElement, NUM_PRESETS};

type Display<DI, SIZE> = Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>>;

impl AppState {
    /// Redraw the whole GUI according to the current application state
    pub fn update_ui<DI: WriteOnlyDataCommand, SIZE: DisplaySize>(
        &self,
        display: &mut Display<DI, SIZE>,
    ) -> Result<(), <Display<DI, SIZE> as DrawTarget>::Error> {
        display.clear(BinaryColor::Off)?;

        // style for unselected cursor
        let stroke_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

        // style for selected cursor
        let thick_stroke_style = PrimitiveStyleBuilder::new()
            .stroke_width(3)
            .stroke_alignment(StrokeAlignment::Inside)
            .stroke_color(BinaryColor::On)
            .build();

        let fill_style = PrimitiveStyle::with_fill(BinaryColor::On);

        // style for RDS text
        let text_style = MonoTextStyle::new(
            &&embedded_graphics::mono_font::iso_8859_2::FONT_6X9,
            BinaryColor::On,
        );

        // style for frequency and preset numbers
        let big_text_style = MonoTextStyle::new(
            &embedded_graphics::mono_font::ascii::FONT_10X20,
            BinaryColor::On,
        );

        // arrows for seek buttons
        let left_arrow = Triangle::new(Point::new(0, 0), Point::new(5, -5), Point::new(5, 5))
            .into_styled(fill_style);
        let right_arrow = Triangle::new(Point::new(5, 0), Point::new(0, -5), Point::new(0, 5))
            .into_styled(fill_style);

        // displays a box around the currently active UI element,
        // if the element is selected, the box is drawn thicker
        let selection_box = |ui_element, x, y, sx, sy, display: &mut _| {
            let style = if self.element_is_active {
                thick_stroke_style
            } else {
                stroke_style
            };

            if self.cursor_at == ui_element {
                RoundedRectangle::with_equal_corners(
                    Rectangle::new(Point::new(x, y), Size::new(sx, sy)),
                    Size::new(3, 3),
                )
                .into_styled(style)
                .draw(display)
            } else {
                Ok(())
            }
        };

        // -- Seek down button --
        selection_box(UIElement::SeekDown, 0, 0, 20, 20, display)?;
        left_arrow.translate(Point::new(4, 9)).draw(display)?;
        left_arrow.translate(Point::new(10, 9)).draw(display)?;

        // -- Frequency setting --
        selection_box(UIElement::FreqControl, 25, 0, 60, 20, display)?;
        let freq = self.freq_khz as f32 / 1000.;
        Text::new(
            format!("{freq:.1}").as_str(),
            Point::new(33, 15),
            big_text_style,
        )
        .draw(display)?;

        // -- Seek up button --
        selection_box(UIElement::SeekUp, 90, 0, 20, 20, display)?;
        right_arrow.translate(Point::new(94, 9)).draw(display)?;
        right_arrow.translate(Point::new(100, 9)).draw(display)?;

        // -- Volume control --
        selection_box(UIElement::VolumeControl, 115, 0, 13, 40, display)?;
        for level in 0..self.volume {
            Rectangle::new(Point::new(117, 34 - level as i32 * 2), Size::new(9, 1))
                .draw_styled(&fill_style, display)?;
        }
        // Level indicator
        selection_box(UIElement::VolumeControl, 109, 45, 19, 19, display)?;
        left_arrow.translate(Point::new(113, 54)).draw(display)?;
        Rectangle::new(Point::new(112, 52), Size::new(5, 5))
            .into_styled(fill_style)
            .draw(display)?;
        for line in 1..=2 {
            Arc::with_center(
                Point::new(118, 53),
                line * 6,
                Angle::from_degrees(-60.),
                Angle::from_degrees(120.),
            )
            .draw_styled(&stroke_style, display)?;
        }

        // station info
        // 19 chars on each line
        let line1 = self.station_info.chars().take(19).collect::<String>();
        let line2 = self
            .station_info
            .chars()
            .skip(19)
            .take(19)
            .collect::<String>();
        Text::new(line1.as_str(), Point::new(2, 30), text_style).draw(display)?;
        Text::new(line2.as_str(), Point::new(2, 40), text_style).draw(display)?;

        // -- Preset stations --
        for preset in 0..NUM_PRESETS {
            let element = UIElement::Preset(preset);
            let preset = preset as i32;
            selection_box(element, preset * 25, 45, 19, 19, display)?;

            Text::new(
                format!("{}", preset + 1).as_str(),
                Point::new(preset * 25 + 5, 60),
                big_text_style,
            )
            .draw(display)?;
        }

        // draw to the display
        display.flush()
    }
}
