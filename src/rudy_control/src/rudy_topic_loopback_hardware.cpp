#include "rudy_control/rudy_topic_loopback_hardware.hpp"

#include <string>
#include <vector>

#include "hardware_interface/types/hardware_interface_type_values.hpp"
#include "pluginlib/class_list_macros.hpp"

namespace rudy_control
{

hardware_interface::CallbackReturn RudyTopicLoopbackHardware::on_init(
  const hardware_interface::HardwareInfo & info)
{
  (void)info;
  info_ = info;
  pos_cmd_.clear();
  pos_state_.clear();
  // Single joint loopback for bring-up tests; replace with URDF-driven joints + topic bridge later.
  pos_cmd_["rudy_loopback_joint"] = 0.0;
  pos_state_["rudy_loopback_joint"] = 0.0;
  return hardware_interface::CallbackReturn::SUCCESS;
}

std::vector<hardware_interface::StateInterface> RudyTopicLoopbackHardware::export_state_interfaces()
{
  std::vector<hardware_interface::StateInterface> out;
  out.emplace_back(hardware_interface::StateInterface(
    "rudy_loopback_joint", hardware_interface::HW_IF_POSITION, &pos_state_["rudy_loopback_joint"]));
  return out;
}

std::vector<hardware_interface::CommandInterface> RudyTopicLoopbackHardware::export_command_interfaces()
{
  std::vector<hardware_interface::CommandInterface> out;
  out.emplace_back(hardware_interface::CommandInterface(
    "rudy_loopback_joint", hardware_interface::HW_IF_POSITION, &pos_cmd_["rudy_loopback_joint"]));
  return out;
}

hardware_interface::return_type RudyTopicLoopbackHardware::read(
  const rclcpp::Time & /*time*/, const rclcpp::Duration & /*period*/)
{
  return hardware_interface::return_type::OK;
}

hardware_interface::return_type RudyTopicLoopbackHardware::write(
  const rclcpp::Time & /*time*/, const rclcpp::Duration & /*period*/)
{
  for (const auto & [name, cmd] : pos_cmd_) {
    pos_state_[name] = cmd;
  }
  return hardware_interface::return_type::OK;
}

}  // namespace rudy_control

PLUGINLIB_EXPORT_CLASS(rudy_control::RudyTopicLoopbackHardware, hardware_interface::SystemInterface)
