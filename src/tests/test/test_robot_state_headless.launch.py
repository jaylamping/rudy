# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Launch test: include headless robot_state + joint_state publishers."""
import unittest

import launch_testing.actions
from launch import LaunchDescription
from launch.actions import IncludeLaunchDescription, TimerAction
from launch.launch_description_sources import AnyLaunchDescriptionSource
from launch.substitutions import PathJoinSubstitution
from launch_ros.substitutions import FindPackageShare


def generate_test_description():
    bringup_launch = PathJoinSubstitution(
        [FindPackageShare("bringup"), "launch", "test_robot_state_headless.launch.xml"]
    )
    return LaunchDescription(
        [
            IncludeLaunchDescription(AnyLaunchDescriptionSource(bringup_launch)),
            TimerAction(period=3.0, actions=[launch_testing.actions.ReadyToTest()]),
        ]
    )


class TestRobotStateHeadless(unittest.TestCase):
    def test_smoke(self):
        self.assertTrue(True)
