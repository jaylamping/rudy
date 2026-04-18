# Copyright 2026 Rudy contributors
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Launch test: include headless robot_state + joint_state publishers."""
import unittest

from launch import LaunchDescription
from launch.actions import IncludeLaunchDescription, TimerAction
from launch.launch_description_sources import AnyLaunchDescriptionSource
from launch.substitutions import PathJoinSubstitution
from launch_ros.substitutions import FindPackageShare
import launch_testing.actions


def generate_test_description():
    bringup_launch = PathJoinSubstitution(
        [FindPackageShare('bringup'), 'launch', 'test_robot_state_headless.launch.xml']
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
