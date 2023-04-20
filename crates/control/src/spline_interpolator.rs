use splines::{Interpolation, Key, Spline};
use thiserror::Error;
use types::{Joints, MotionFile};

use std::{fmt::Debug, time::Duration};

pub struct SplineInterpolator {
    spline: Spline<f32, Joints<f32>>,
    current_time: Duration,
    end_time: Duration,
}

pub trait MapArgumentExt<FromArgument, ToArgument, Value> {
    fn map_argument(self) -> Result<Interpolation<ToArgument, Value>, InterpolatorError>;
}

impl<FromArgument: Debug, ToArgument, Joints: Debug>
    MapArgumentExt<FromArgument, ToArgument, Joints> for Interpolation<FromArgument, Joints>
{
    fn map_argument(self) -> Result<Interpolation<ToArgument, Joints>, InterpolatorError> {
        match self {
            Interpolation::Linear => Ok(Interpolation::Linear),
            Interpolation::Cosine => Ok(Interpolation::Cosine),
            Interpolation::CatmullRom => Ok(Interpolation::CatmullRom),
            unimplemented_mode => Err(InterpolatorError::UnsupportedInterpolationMode {
                interpolation_mode: format!("{unimplemented_mode:?}"),
            }),
        }
    }
}

#[derive(Error, Debug)]
pub enum InterpolatorError {
    #[error("cannot perform {interpolation_mode} with {keys_before} keys before and {keys_after} keys after")]
    InterpolationControlKeyError {
        interpolation_mode: String,
        keys_before: usize,
        keys_after: usize,
    },
    #[error("need at least two keys to create an interpolator")]
    TooFewKeysError,
    #[error("uses unsupported interpolation mode {interpolation_mode}")]
    UnsupportedInterpolationMode { interpolation_mode: String },
}

impl InterpolatorError {
    fn create_control_key_error(
        keys: &[Key<f32, Joints<f32>>],
        current_time: Duration,
    ) -> InterpolatorError {
        let current_control_key = keys
            .iter()
            .filter(|key| key.t < current_time.as_secs_f32())
            .last()
            .unwrap();
        let current_interpolation_mode = current_control_key.interpolation;

        let prior_control_points = keys
            .iter()
            .take_while(|key| key.t != current_control_key.t)
            .count();
        let following_control_points = keys.len() - 1 - prior_control_points;

        InterpolatorError::InterpolationControlKeyError {
            interpolation_mode: format!("{current_interpolation_mode:?}"),
            keys_before: prior_control_points,
            keys_after: following_control_points,
        }
    }
}

impl TryFrom<MotionFile> for SplineInterpolator {
    type Error = InterpolatorError;

    fn try_from(motion_file: MotionFile) -> Result<Self, InterpolatorError> {
        let mut current_time = Duration::ZERO;
        let mut keys = vec![Key::new(
            current_time,
            motion_file.initial_positions,
            Interpolation::Linear,
        )];

        keys.extend(motion_file.frames.into_iter().map(|frame| {
            current_time += frame.duration;
            Key::new(current_time, frame.positions, Interpolation::Linear)
        }));

        SplineInterpolator::try_new(keys)
    }
}

impl SplineInterpolator {
    pub fn try_new(mut keys: Vec<Key<Duration, Joints<f32>>>) -> Result<Self, InterpolatorError> {
        if keys.len() < 2 {
            return Err(InterpolatorError::TooFewKeysError);
        }

        keys.sort_unstable_by_key(|key| key.t);

        let current_time = Duration::ZERO;
        let start_time = keys.first().unwrap().t;
        let end_time = keys.last().unwrap().t - start_time;
        let last_key_index = keys.len() - 1;

        let mut spline = Spline::from_vec(
            keys.into_iter()
                .map(|key| {
                    Ok(Key::new(
                        key.t.as_secs_f32() - start_time.as_secs_f32(),
                        key.value,
                        key.interpolation.map_argument()?,
                    ))
                })
                .collect::<Result<_, _>>()?,
        );

        spline.add(Self::create_zero_gradient(
            &spline.keys()[last_key_index],
            &spline.keys()[last_key_index - 1],
        ));
        spline.add(Self::create_zero_gradient(
            &spline.keys()[0],
            &spline.keys()[1],
        ));

        Ok(Self {
            spline,
            current_time,
            end_time,
        })
    }

    fn create_zero_gradient(
        key_center: &Key<f32, Joints<f32>>,
        key_other: &Key<f32, Joints<f32>>,
    ) -> Key<f32, Joints<f32>> {
        Key::new(
            2. * key_center.t - key_other.t,
            key_other.value,
            key_center.interpolation,
        )
    }

    pub fn advance_by(&mut self, time_step: Duration) {
        self.current_time += time_step;
    }

    pub fn value(&self) -> Result<Joints<f32>, InterpolatorError> {
        if self.current_time >= self.end_time {
            self.spline.keys().iter().rev().nth(1).map(|key| key.value)
        } else {
            self.spline.sample(self.current_time.as_secs_f32())
        }
        .ok_or_else(|| {
            InterpolatorError::create_control_key_error(self.spline.keys(), self.current_time)
        })
    }

    pub fn is_finished(&self) -> bool {
        self.current_time >= self.end_time
    }

    pub fn reset(&mut self) {
        self.current_time = Duration::ZERO;
    }
}
