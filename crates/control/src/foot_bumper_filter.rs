use color_eyre::Result;
use context_attribute::context;
use framework::{AdditionalOutput, MainOutput};
use nalgebra::{point, Point2};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, SystemTime};
use types::{
    cycle_time::CycleTime, fall_state::FallState, foot_bumper_obstacle::FootBumperObstacle,
    foot_bumper_values::FootBumperValues, sensor_data::SensorData,
};

#[derive(Default, Deserialize, Serialize)]
pub struct FootBumperFilter {
    left_point: Point2<f32>,
    right_point: Point2<f32>,
    middle_point: Point2<f32>,
    left_in_use: bool,
    right_in_use: bool,
    left_detection_buffer: VecDeque<bool>,
    right_detection_buffer: VecDeque<bool>,
    left_count: i32,
    right_count: i32,
    last_left_time: Option<SystemTime>,
    last_right_time: Option<SystemTime>,
    left_pressed_last_cycle: bool,
    right_pressed_last_cycle: bool,
}

#[context]
pub struct CreationContext {
    pub buffer_size: Parameter<usize, "foot_bumper_filter.buffer_size">,
    pub obstacle_distance: Parameter<f32, "foot_bumper_filter.obstacle_distance">,
    pub sensor_angle: Parameter<f32, "foot_bumper_filter.sensor_angle">,
}

#[context]
pub struct CycleContext {
    pub acceptance_duration: Parameter<Duration, "foot_bumper_filter.acceptance_duration">,
    pub activations_needed: Parameter<i32, "foot_bumper_filter.activations_needed">,
    pub enabled: Parameter<bool, "obstacle_filter.use_foot_bumper_measurements">,
    pub number_of_detections_in_buffer_for_defective_declaration: Parameter<
        usize,
        "foot_bumper_filter.number_of_detections_in_buffer_for_defective_declaration",
    >,
    pub number_of_detections_in_buffer_to_reset_in_use:
        Parameter<usize, "foot_bumper_filter.number_of_detections_in_buffer_to_reset_in_use">,
    pub obstacle_distance: Parameter<f32, "foot_bumper_filter.obstacle_distance">,
    pub sensor_angle: Parameter<f32, "foot_bumper_filter.sensor_angle">,

    pub cycle_time: Input<CycleTime, "cycle_time">,
    pub fall_state: Input<FallState, "fall_state">,
    pub sensor_data: Input<SensorData, "sensor_data">,

    pub foot_bumper_values: AdditionalOutput<FootBumperValues, "foot_bumper_values">,
}

#[context]
#[derive(Default)]
pub struct MainOutputs {
    pub foot_bumper_obstacle: MainOutput<Vec<FootBumperObstacle>>,
}

impl FootBumperFilter {
    pub fn new(context: CreationContext) -> Result<Self> {
        let left_point = point![
            context.sensor_angle.cos() * *context.obstacle_distance,
            context.sensor_angle.sin() * *context.obstacle_distance
        ];
        let right_point = point![
            context.sensor_angle.cos() * *context.obstacle_distance,
            -context.sensor_angle.sin() * *context.obstacle_distance
        ];
        let middle_point = point![*context.obstacle_distance, 0.0];
        Ok(Self {
            left_point,
            right_point,
            middle_point,
            left_in_use: true,
            right_in_use: true,
            left_detection_buffer: VecDeque::from(vec![false; *context.buffer_size]),
            right_detection_buffer: VecDeque::from(vec![false; *context.buffer_size]),
            ..Default::default()
        })
    }

    pub fn cycle(&mut self, mut context: CycleContext) -> Result<MainOutputs> {
        let fall_state = context.fall_state;

        if !context.enabled {
            return Ok(MainOutputs::default());
        }

        let touch_sensors = context.sensor_data.touch_sensors;
        if touch_sensors.left_foot_left || touch_sensors.left_foot_right {
            if !self.left_pressed_last_cycle {
                self.left_count += 1;
                self.left_pressed_last_cycle = true;
                self.last_left_time = Some(SystemTime::now());
            }
        } else {
            self.left_pressed_last_cycle = false;
        }

        if touch_sensors.right_foot_left || touch_sensors.right_foot_right {
            if !self.right_pressed_last_cycle {
                self.right_count += 1;
                self.right_pressed_last_cycle = true;
                self.last_right_time = Some(SystemTime::now());
            }
        } else {
            self.right_pressed_last_cycle = false;
        }

        if let Some(last_left_foot_bumper_time) = self.last_left_time {
            if last_left_foot_bumper_time
                .elapsed()
                .expect("Time ran backwards")
                > *context.acceptance_duration
            {
                self.last_left_time = None;
                self.left_count = 0;
                self.left_pressed_last_cycle = false;
            }
        }

        if let Some(last_right_foot_bumper_time) = self.last_right_time {
            if last_right_foot_bumper_time
                .elapsed()
                .expect("Time ran backwards")
                > *context.acceptance_duration
            {
                self.last_right_time = None;
                self.right_count = 0;
                self.right_pressed_last_cycle = false;
            }
        }
        self.left_detection_buffer
            .push_back(self.left_pressed_last_cycle);
        self.left_detection_buffer.pop_front();
        self.right_detection_buffer
            .push_back(self.right_pressed_last_cycle);
        self.right_detection_buffer.pop_front();

        let obstacle_detected_on_left = self.left_count >= *context.activations_needed;
        let obstacle_detected_on_right = self.right_count >= *context.activations_needed;

        self.check_for_bumper_errors(&context);

        let obstacle_positions = match (
            fall_state,
            obstacle_detected_on_left,
            obstacle_detected_on_right,
            self.left_in_use,
            self.right_in_use,
        ) {
            (FallState::Upright, true, true, true, true) => vec![self.middle_point],
            (FallState::Upright, true, false, true, _) => vec![self.left_point],
            (FallState::Upright, false, true, _, true) => vec![self.right_point],
            _ => vec![],
        };
        let foot_bumper_obstacles: Vec<_> = obstacle_positions
            .iter()
            .map(|position_in_robot| FootBumperObstacle {
                position_in_robot: *position_in_robot,
            })
            .collect();

        context
            .foot_bumper_values
            .fill_if_subscribed(|| FootBumperValues {
                left_foot_bumper_count: self.left_count,
                right_foot_bumper_count: self.right_count,
                obstacle_deteced_on_left: obstacle_detected_on_left,
                obstacle_deteced_on_right: obstacle_detected_on_right,
            });

        Ok(MainOutputs {
            foot_bumper_obstacle: foot_bumper_obstacles.into(),
        })
    }

    fn check_for_bumper_errors(&mut self, context: &CycleContext) {
        let left_count: usize = self.left_detection_buffer.iter().filter(|x| **x).count();

        if left_count >= *context.number_of_detections_in_buffer_for_defective_declaration {
            self.left_in_use = false;
        }
        let right_count: usize = self.right_detection_buffer.iter().filter(|x| **x).count();

        if right_count >= *context.number_of_detections_in_buffer_for_defective_declaration {
            self.right_in_use = false;
        }
        if !self.left_in_use
            && left_count <= *context.number_of_detections_in_buffer_to_reset_in_use
        {
            self.left_in_use = true;
        }
        if !self.right_in_use
            && right_count <= *context.number_of_detections_in_buffer_to_reset_in_use
        {
            self.right_in_use = true;
        }
    }
}
