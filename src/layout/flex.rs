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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_new() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn test_size_zero() {
        let size = Size::zero();
        assert_eq!(size.width, 0.0);
        assert_eq!(size.height, 0.0);
    }

    #[test]
    fn test_size_is_empty() {
        assert!(Size::zero().is_empty());
        assert!(Size::new(0.0, 10.0).is_empty());
        assert!(Size::new(10.0, 0.0).is_empty());
        assert!(Size::new(-5.0, 10.0).is_empty());
        assert!(!Size::new(10.0, 10.0).is_empty());
    }

    #[test]
    fn test_size_default() {
        let size = Size::default();
        assert_eq!(size, Size::zero());
    }

    #[test]
    fn test_constraints_new() {
        let c = Constraints::new(10.0, 20.0, 100.0, 200.0);
        assert_eq!(c.min_width, 10.0);
        assert_eq!(c.min_height, 20.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.max_height, 200.0);
    }

    #[test]
    fn test_constraints_tight() {
        let size = Size::new(50.0, 75.0);
        let c = Constraints::tight(size);
        assert_eq!(c.min_width, 50.0);
        assert_eq!(c.min_height, 75.0);
        assert_eq!(c.max_width, 50.0);
        assert_eq!(c.max_height, 75.0);
        assert!(c.is_tight());
    }

    #[test]
    fn test_constraints_loose() {
        let size = Size::new(100.0, 150.0);
        let c = Constraints::loose(size);
        assert_eq!(c.min_width, 0.0);
        assert_eq!(c.min_height, 0.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.max_height, 150.0);
    }

    #[test]
    fn test_constraints_unbounded() {
        let c = Constraints::unbounded();
        assert_eq!(c.min_width, 0.0);
        assert_eq!(c.min_height, 0.0);
        assert_eq!(c.max_width, f32::INFINITY);
        assert_eq!(c.max_height, f32::INFINITY);
    }

    #[test]
    fn test_constraints_constrain() {
        let c = Constraints::new(10.0, 20.0, 100.0, 200.0);

        // Within bounds
        let size = Size::new(50.0, 50.0);
        assert_eq!(c.constrain(size), size);

        // Below min
        let size = Size::new(5.0, 15.0);
        assert_eq!(c.constrain(size), Size::new(10.0, 20.0));

        // Above max
        let size = Size::new(150.0, 250.0);
        assert_eq!(c.constrain(size), Size::new(100.0, 200.0));

        // Mixed
        let size = Size::new(5.0, 250.0);
        assert_eq!(c.constrain(size), Size::new(10.0, 200.0));
    }

    #[test]
    fn test_constraints_is_tight() {
        let tight = Constraints::tight(Size::new(50.0, 50.0));
        assert!(tight.is_tight());

        let loose = Constraints::loose(Size::new(50.0, 50.0));
        assert!(!loose.is_tight());

        let custom = Constraints::new(10.0, 10.0, 50.0, 50.0);
        assert!(!custom.is_tight());
    }

    #[test]
    fn test_constraints_max_size() {
        let c = Constraints::new(10.0, 20.0, 100.0, 200.0);
        assert_eq!(c.max_size(), Size::new(100.0, 200.0));
    }

    #[test]
    fn test_constraints_default() {
        let c = Constraints::default();
        assert_eq!(c, Constraints::unbounded());
    }
}
