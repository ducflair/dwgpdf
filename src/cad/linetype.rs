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

    /// Converts line type name (e.g. "DASHED", "CENTER", "HIDDEN", "CONTINUOUS")
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
        } else if upper.contains("DASHED") {
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
        } else if upper.contains("CENTER") {
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
