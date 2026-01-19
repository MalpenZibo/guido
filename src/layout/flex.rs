#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub const fn zero() -> Self {
        Self {
            width: 0.0,
            height: 0.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

impl Default for Size {
    fn default() -> Self {
        Self::zero()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraints {
    pub min_width: f32,
    pub min_height: f32,
    pub max_width: f32,
    pub max_height: f32,
}

impl Constraints {
    pub fn new(min_width: f32, min_height: f32, max_width: f32, max_height: f32) -> Self {
        Self {
            min_width,
            min_height,
            max_width,
            max_height,
        }
    }

    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            min_height: size.height,
            max_width: size.width,
            max_height: size.height,
        }
    }

    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            min_height: 0.0,
            max_width: size.width,
            max_height: size.height,
        }
    }

    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            min_height: 0.0,
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }

    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: size.width.max(self.min_width).min(self.max_width),
            height: size.height.max(self.min_height).min(self.max_height),
        }
    }

    pub fn max_size(&self) -> Size {
        Size {
            width: self.max_width,
            height: self.max_height,
        }
    }

    pub fn is_tight(&self) -> bool {
        self.min_width == self.max_width && self.min_height == self.max_height
    }
}

impl Default for Constraints {
    fn default() -> Self {
        Self::unbounded()
    }
}
