//! Synthesized first baselines for the admitted horizontal-LTR column case.
//!
//! Taffy 0.14 substitutes individual flex-start alignment for column baselines.
//! CSS Flexbox 1 section 8.3 instead aligns the items' synthesized border-box
//! baselines as a group. CSS Align 3 section 9.1 assumes vertical-lr for these
//! horizontal-LTR items, putting that synthetic baseline on the left border.
//! https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline
//! https://www.w3.org/TR/css-align-3/#baseline-export
//!
//! No hidden Taffy line membership is guessed from coincident coordinates.
//! Finite-height wrapped groups and unequal left margins remain explicit
//! limitations. Equal left margins guarantee that the group's intrinsic width
//! is the same maximum outer width that Taffy already measures: no sizing pass,
//! allocation, or descendant relayout is added here.

use super::{Adapter, Kind, MAX_EXTENT};
use crate::style::{AlignKeyword, Display, FlexDirection, FlexWrap, Length, Position};

impl Adapter<'_, '_, '_> {
    fn column_baseline_margin(&self, index: usize) -> Option<Length> {
        let root = &self.layout.styles[self.slots[0].node];
        let slot = &self.slots[index];
        let (alignment, margins) = match slot.kind {
            Kind::Element => {
                let style = &self.layout.styles[slot.node];
                let alignment = if style.layout.align_self.keyword == AlignKeyword::Auto {
                    root.layout.align_items.keyword
                } else {
                    style.layout.align_self.keyword
                };
                (alignment, style.margin)
            }
            Kind::Anonymous { .. } => (root.layout.align_items.keyword, [Length::Px(0.0); 4]),
            Kind::Root => return None,
        };
        (alignment == AlignKeyword::Baseline
            && margins[1] != Length::Auto
            && margins[3] != Length::Auto)
            .then_some(margins[3])
    }

    fn column_baseline_work(&mut self, passes: usize) -> Option<()> {
        // Admission already caps this at 512 items. Prepay every scan even if
        // an early rejection visits only a prefix; never emit a partial scene.
        let work = (self.slots.len() - 1).checked_mul(passes)?;
        if work > self.layout.budget {
            self.layout.fail(
                self.slots[0].node,
                "column baseline alignment work budget exhausted",
            );
            return None;
        }
        self.layout.budget -= work;
        Some(())
    }

    pub(super) fn prepare_column_baseline(&mut self, height: Option<f32>) -> Option<bool> {
        let root = &self.layout.styles[self.slots[0].node];
        if !matches!(root.display, Display::Flex | Display::InlineFlex)
            || !matches!(
                root.layout.flex_direction,
                FlexDirection::Column | FlexDirection::ColumnReverse
            )
        {
            return Some(false);
        }
        let wrap = root.layout.flex_wrap;
        self.column_baseline_work(1)?;
        let mut count = 0;
        let mut margin = None;
        let mut unequal_margins = false;
        for index in 1..self.slots.len() {
            if let Some(left) = self.column_baseline_margin(index) {
                count += 1;
                unequal_margins |= margin.is_some_and(|previous| previous != left);
                margin = Some(left);
            }
        }
        if count < 2 {
            return Some(false);
        }
        if unequal_margins {
            self.layout.fail(
                self.slots[0].node,
                "column baseline groups with unequal left margins require additional intrinsic sizing",
            );
            return None;
        }
        // RootContent removes the outer min/max constraints. With no supplied
        // height, this Taffy context has MaxContent main-axis available space
        // and cannot create a second column, even for wrap/wrap-reverse.
        if wrap != FlexWrap::NoWrap && height.is_some() {
            self.layout.fail(
                self.slots[0].node,
                "column baseline groups with finite-height wrapping require per-line alignment",
            );
            return None;
        }
        Some(true)
    }

    fn column_relative_x(&self, index: usize, width: Option<f32>) -> f32 {
        if !matches!(self.slots[index].kind, Kind::Element) {
            return 0.0;
        }
        let style = &self.layout.styles[self.slots[index].node];
        if style.layout.position != Position::Relative {
            return 0.0;
        }
        // Match Taffy's resolution against the original definite inner width,
        // not its later intrinsic width. Percentages are auto when indefinite.
        let resolve = |value| match value {
            Length::Px(value) => Some(value),
            Length::Percent(value) => width.map(|width| value * width),
            Length::Auto => None,
        };
        resolve(style.layout.inset[3])
            .or_else(|| resolve(style.layout.inset[1]).map(|value| -value))
            .unwrap_or(0.0)
    }

    pub(super) fn align_column_baseline(&mut self, width: Option<f32>) -> Option<()> {
        self.column_baseline_work(2)?;
        let reverse =
            self.layout.styles[self.slots[0].node].layout.flex_wrap == FlexWrap::WrapReverse;
        let mut baseline: Option<f32> = None;
        for index in 1..self.slots.len() {
            if self.column_baseline_margin(index).is_some() {
                let original = self.slots[index].geometry.location.x;
                let x = original - self.column_relative_x(index, width);
                if !original.is_finite()
                    || original.abs() > MAX_EXTENT
                    || !x.is_finite()
                    || x.abs() > MAX_EXTENT
                {
                    self.layout.fail(
                        self.slots[index].node,
                        "column baseline input is outside finite extent",
                    );
                    return None;
                }
                baseline = Some(baseline.map_or(x, |previous| {
                    if reverse {
                        previous.min(x)
                    } else {
                        previous.max(x)
                    }
                }));
            }
        }
        let baseline = baseline?;
        for index in 1..self.slots.len() {
            if self.column_baseline_margin(index).is_some() {
                self.slots[index].geometry.location.x =
                    baseline + self.column_relative_x(index, width);
            }
        }
        Some(())
    }
}
