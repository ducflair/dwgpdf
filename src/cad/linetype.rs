/// Standard CAD LineType to PDF dash pattern mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct LineDashPattern {
    pub dash_array: Vec<f32>,
    pub phase: f32,
}

impl LineDashPattern {
    pub fn solid() -> Self {
        Self {
            dash_array: Vec::new(),
            phase: 0.0,
        }
    }

    pub fn is_solid(&self) -> bool {
        self.dash_array.is_empty()
    }

    /// Converts CAD linetype elements to PDF dash pattern.
    pub fn from_elements(elements: &[acadrust::tables::LineTypeElement], scale: f32) -> Self {
        if elements.is_empty() {
            return Self::solid();
        }
        let mut s = scale.max(0.001);
        let raw_period: f32 = elements.iter().map(|e| e.length.abs() as f32).sum();
        if raw_period > 0.0 && raw_period * s < 4.0 {
            s = 4.0 / raw_period;
        }
        let dashes: Vec<f32> = elements
            .iter()
            .map(|e| {
                let d = e.length.abs() as f32;
                if d == 0.0 {
                    0.0
                } else {
                    (d * s).max(0.3)
                }
            })
            .collect();
        Self {
            dash_array: dashes,
            phase: 0.0,
        }
    }

    /// Converts line type name (e.g. "DASHED", "CENTER", "HIDDEN", "CONTINUOUS", "1mm DASH")
    /// to PDF dash pattern, scaled by scale factor.
    pub fn from_name(name: &str, scale: f32) -> Self {
        let upper = name.to_uppercase();
        let s = scale.max(0.1);

        if upper.contains("CONTINUOUS") || upper.is_empty() || upper == "BYLAYER" || upper == "BYBLOCK" {
            Self::solid()
        } else if upper.contains("DASHDOT") {
            Self {
                dash_array: vec![10.0 * s, 3.0 * s, 2.0 * s, 3.0 * s],
                phase: 0.0,
            }
        } else if upper.contains("DASH") || upper.contains("TRAC") || upper.contains("TRAÇ") {
            Self {
                dash_array: vec![8.0 * s, 4.0 * s],
                phase: 0.0,
            }
        } else if upper.contains("DOT") {
            Self {
                dash_array: vec![2.0 * s, 3.0 * s],
                phase: 0.0,
            }
        } else if upper.contains("HIDDEN") {
            Self {
                dash_array: vec![4.0 * s, 2.0 * s],
                phase: 0.0,
            }
        } else if upper.contains("CENTER") || upper.contains("AXIS") || upper.contains("EIXO") {
            Self {
                dash_array: vec![12.0 * s, 3.0 * s, 3.0 * s, 3.0 * s],
                phase: 0.0,
            }
        } else if upper.contains("PHANTOM") {
            Self {
                dash_array: vec![12.0 * s, 3.0 * s, 3.0 * s, 3.0 * s, 3.0 * s, 3.0 * s],
                phase: 0.0,
            }
        } else if upper.contains("BORDER") {
            Self {
                dash_array: vec![10.0 * s, 3.0 * s, 10.0 * s, 3.0 * s, 3.0 * s, 3.0 * s],
                phase: 0.0,
            }
        } else if upper.contains("DIVIDE") {
            Self {
                dash_array: vec![8.0 * s, 3.0 * s, 2.0 * s, 3.0 * s, 2.0 * s, 3.0 * s],
                phase: 0.0,
            }
        } else {
            Self::solid()
        }
    }
}
