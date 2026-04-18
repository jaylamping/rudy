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

// See `control/topic_loopback_hardware.hpp` for the rationale on why this plugin is C++ rather
// than Rust (short version: `controller_manager` loads `SystemInterface` subclasses through
// `pluginlib`/`dlopen` and `rclrs` has no equivalent in ROS 2 Jazzy).

#include "control/topic_loopback_hardware.hpp"

#include <string>
#include <vector>

#include "hardware_interface/types/hardware_interface_type_values.hpp"
#include "pluginlib/class_list_macros.hpp"

namespace control
{

hardware_interface::CallbackReturn TopicLoopbackHardware::on_init(
  const hardware_interface::HardwareInfo & info)
{
  (void)info;
  info_ = info;
  pos_cmd_.clear();
  pos_state_.clear();
  // Single joint loopback for bring-up tests; replace with URDF-driven joints + topic bridge later.
  pos_cmd_["loopback_joint"] = 0.0;
  pos_state_["loopback_joint"] = 0.0;
  return hardware_interface::CallbackReturn::SUCCESS;
}

std::vector<hardware_interface::StateInterface> TopicLoopbackHardware::export_state_interfaces()
{
  std::vector<hardware_interface::StateInterface> out;
  out.emplace_back(hardware_interface::StateInterface(
    "loopback_joint", hardware_interface::HW_IF_POSITION, &pos_state_["loopback_joint"]));
  return out;
}

std::vector<hardware_interface::CommandInterface> TopicLoopbackHardware::export_command_interfaces()
{
  std::vector<hardware_interface::CommandInterface> out;
  out.emplace_back(hardware_interface::CommandInterface(
    "loopback_joint", hardware_interface::HW_IF_POSITION, &pos_cmd_["loopback_joint"]));
  return out;
}

hardware_interface::return_type TopicLoopbackHardware::read(
  const rclcpp::Time & /*time*/, const rclcpp::Duration & /*period*/)
{
  return hardware_interface::return_type::OK;
}

hardware_interface::return_type TopicLoopbackHardware::write(
  const rclcpp::Time & /*time*/, const rclcpp::Duration & /*period*/)
{
  for (const auto & [name, cmd] : pos_cmd_) {
    pos_state_[name] = cmd;
  }
  return hardware_interface::return_type::OK;
}

}  // namespace control

PLUGINLIB_EXPORT_CLASS(control::TopicLoopbackHardware, hardware_interface::SystemInterface)
