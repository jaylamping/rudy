// Copyright 2026 Rudy contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#include <gtest/gtest.h>

#include "control/topic_loopback_hardware.hpp"
#include "hardware_interface/hardware_info.hpp"

TEST(TopicLoopbackHardware, exports_interfaces_with_empty_joints)
{
  control::TopicLoopbackHardware hw;
  hardware_interface::HardwareInfo info;
  info.name = "test";
  info.type = "system";
  ASSERT_EQ(hw.on_init(info), hardware_interface::CallbackReturn::SUCCESS);

  auto states = hw.export_state_interfaces();
  auto cmds = hw.export_command_interfaces();
  ASSERT_EQ(states.size(), 1u);
  ASSERT_EQ(cmds.size(), 1u);
}
