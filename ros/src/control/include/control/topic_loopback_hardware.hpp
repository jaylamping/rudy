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

#ifndef CONTROL__TOPIC_LOOPBACK_HARDWARE_HPP_
#define CONTROL__TOPIC_LOOPBACK_HARDWARE_HPP_

#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include "hardware_interface/handle.hpp"
#include "hardware_interface/hardware_info.hpp"
#include "hardware_interface/system_interface.hpp"
#include "hardware_interface/types/hardware_interface_return_values.hpp"
#include "rclcpp/duration.hpp"
#include "rclcpp/macros.hpp"
#include "rclcpp/time.hpp"

namespace control
{

/// Minimal `SystemInterface` that mirrors commanded position into reported state (loopback).
///
/// ## Why is this in C++ instead of Rust?
///
/// `controller_manager` discovers and instantiates hardware interfaces through `pluginlib`,
/// which uses `dlopen` plus a registered C++ class loader (`PLUGINLIB_EXPORT_CLASS`) to
/// construct subclasses of `hardware_interface::SystemInterface` at runtime. The plugin must
/// therefore be a C++ shared library implementing that base class — no other language can
/// satisfy the loader contract.
///
/// In ROS 2 Jazzy there is no first-party Rust path to replace this:
///   - `rclrs` is alpha and ships no `pluginlib`, `hardware_interface`, `controller_interface`,
///     or `controller_manager` bindings.
///   - The standard pattern for "Rust + ros2_control" is exactly what `docs/architecture.md`
///     describes: keep a thin C++ `SystemInterface` shim, and bridge commands/state to a Rust
///     process (`driver_node`) over ROS topics. Everything else in this repo (`driver`,
///     `rudydae`, the RobStride CAN stack) is already Rust.
///
/// This class is the loopback variant of that shim — used today for CI smoke + bring-up. The
/// next iteration adds a topic publisher/subscriber pair and renames it accordingly; the C++
/// surface stays roughly this small (~60 lines) because all real logic lives in `driver`.
class TopicLoopbackHardware : public hardware_interface::SystemInterface
{
public:
  RCLCPP_SHARED_PTR_DEFINITIONS(TopicLoopbackHardware)

  hardware_interface::CallbackReturn on_init(
    const hardware_interface::HardwareInfo & info) override;

  std::vector<hardware_interface::StateInterface> export_state_interfaces() override;

  std::vector<hardware_interface::CommandInterface> export_command_interfaces() override;

  hardware_interface::return_type read(
    const rclcpp::Time & time,
    const rclcpp::Duration & period) override;

  hardware_interface::return_type write(
    const rclcpp::Time & time,
    const rclcpp::Duration & period) override;

private:
  hardware_interface::HardwareInfo info_;
  std::unordered_map<std::string, double> pos_cmd_;
  std::unordered_map<std::string, double> pos_state_;
};

}  // namespace control

#endif  // CONTROL__TOPIC_LOOPBACK_HARDWARE_HPP_
