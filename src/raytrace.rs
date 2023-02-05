use mcprotocol::common::play::{BlockPos, Location, SimpleLocation};
use std::ops::{Mul, MulAssign};

pub struct Vec3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3D {
    pub fn length(&self) -> f64 {
        f64::sqrt(self.x * self.x + self.y * self.y + self.z * self.z)
    }

    pub fn normalize(&mut self) {
        let length = self.length();
        self.x /= length;
        self.y /= length;
        self.z /= length;
    }
}

impl Mul<f64> for Vec3D {
    type Output = Vec3D;

    fn mul(self, rhs: f64) -> Self::Output {
        Vec3D {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}

impl MulAssign<f64> for Vec3D {
    fn mul_assign(&mut self, rhs: f64) {
        self.x *= rhs;
        self.y *= rhs;
        self.z *= rhs;
    }
}

pub struct RayTraceIterator {
    pub empty: bool,
    pub has_next_block: bool,
    pub delta: (i32, i32, i32),
    pub delta_div: (f64, f64, f64),
    pub local: (i32, i32, i32),
    pub frac: (f64, f64, f64),
}

pub fn direction_from_yaw_pitch(yaw: f32, pitch: f32) -> Vec3D {
    let yaw = yaw.to_radians() as f64;
    let pitch = pitch.to_radians() as f64;

    let xz = pitch.cos();
    let x = -xz * yaw.sin();
    let z = xz * yaw.cos();
    let y = -pitch.sin();

    Vec3D { x, y, z }
}

impl RayTraceIterator {
    const LERP_CONSTANT: f64 = -1.0E-7;

    pub fn new(from: Location, max_distance: f64) -> Self {
        let mut direction = direction_from_yaw_pitch(from.yaw, from.pitch);
        let from = from.inner_loc;

        direction.normalize();
        direction *= max_distance;
        let end = SimpleLocation {
            x: from.x + direction.x,
            y: from.y + direction.y,
            z: from.z + direction.z,
        };

        let l_ets_x = Self::lerp(end.x, from.x);
        let l_ets_y = Self::lerp(end.y, from.y);
        let l_ets_z = Self::lerp(end.z, from.z);
        let l_ste_x = Self::lerp(from.x, end.x);
        let l_ste_y = Self::lerp(from.y, end.y);
        let l_ste_z = Self::lerp(from.z, end.z);
        let local_x = Self::floor(l_ste_x);
        let local_y = Self::floor(l_ste_y);
        let local_z = Self::floor(l_ste_z);
        let empty = from.eq(&end);
        let has_next_block = !empty;
        if has_next_block {
            let delta_x_lerp = l_ets_x - l_ste_x;
            let delta_y_lerp = l_ets_y - l_ste_y;
            let delta_z_lerp = l_ets_z - l_ste_z;
            let ax = Self::sign(delta_x_lerp);
            let ay = Self::sign(delta_y_lerp);
            let az = Self::sign(delta_z_lerp);
            let ax_div = if ax == 0 {
                f64::MAX
            } else {
                ax as f64 / delta_x_lerp
            };
            let ay_div = if ay == 0 {
                f64::MAX
            } else {
                ay as f64 / delta_y_lerp
            };
            let az_div = if az == 0 {
                f64::MAX
            } else {
                az as f64 / delta_z_lerp
            };
            let frac_x = ax_div
                * if ax > 0 {
                    1.0 - Self::frac(l_ste_x)
                } else {
                    Self::frac(l_ste_x)
                };
            let frac_y = ay_div
                * if ay > 0 {
                    1.0 - Self::frac(l_ste_y)
                } else {
                    Self::frac(l_ste_y)
                };
            let frac_z = az_div
                * if az > 0 {
                    1.0 - Self::frac(l_ste_z)
                } else {
                    Self::frac(l_ste_z)
                };
            Self {
                empty,
                has_next_block,
                delta: (ax, ay, az),
                delta_div: (ax_div, ay_div, az_div),
                local: (local_x, local_y, local_z),
                frac: (frac_x, frac_y, frac_z),
            }
        } else {
            Self {
                empty,
                has_next_block,
                delta: (-1, -1, -1),
                delta_div: (-1.0, -1.0, -1.0),
                local: (local_x, local_y, local_z),
                frac: (-1.0, -1.0, -1.0),
            }
        }
    }

    fn lerp(start: f64, end: f64) -> f64 {
        start + Self::LERP_CONSTANT * (end - start)
    }

    fn floor(value: f64) -> i32 {
        let floor = value as i32;
        if value < floor as f64 {
            floor - 1
        } else {
            floor
        }
    }

    fn sign(value: f64) -> i32 {
        if value == 0.0 {
            0
        } else if value > 0.0 {
            1
        } else {
            -1
        }
    }

    fn frac(value: f64) -> f64 {
        value - Self::lfloor(value) as f64
    }

    fn lfloor(value: f64) -> i64 {
        let l = value as i64;
        if value < l as f64 {
            l - 1
        } else {
            l
        }
    }
}

impl Iterator for RayTraceIterator {
    type Item = BlockPos;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_next_block {
            return None;
        }

        let temp = BlockPos {
            x: self.local.0,
            y: self.local.1,
            z: self.local.2,
        };

        if self.frac.0 > 1.0 && self.frac.1 > 1.0 && self.frac.2 > 1.0 {
            self.has_next_block = false;
            self.empty = true;
            return Some(temp);
        }

        if self.frac.0 < self.frac.1 {
            if self.frac.0 < self.frac.2 {
                self.local.0 += self.delta.0;
                self.frac.0 += self.delta_div.0;
            } else {
                self.local.2 += self.delta.2;
                self.frac.2 += self.delta_div.2;
            }
        } else if self.frac.1 < self.frac.2 {
            self.local.1 += self.delta.1;
            self.frac.1 += self.delta_div.1;
        } else {
            self.local.2 += self.delta.2;
            self.frac.2 += self.delta_div.2;
        }

        return Some(temp);
    }
}

pub struct StepIterator {
    current: f64,
    step: f64,
    end: f64,
}

impl StepIterator {
    pub fn new(start: f64, step: f64, end: f64) -> Self {
        Self {
            current: start,
            step,
            end,
        }
    }
}

impl Iterator for StepIterator {
    type Item = f64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current > self.end {
            return None;
        }
        let temp = self.current;
        self.current += self.step;
        Some(temp)
    }
}
