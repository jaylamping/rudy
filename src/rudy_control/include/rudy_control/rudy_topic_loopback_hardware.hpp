#ifndef RUDY_CONTROL__RUDY_TOPIC_LOOPBACK_HARDWARE_HPP_
#define RUDY_CONTROL__RUDY_TOPIC_LOOPBACK_HARDWARE_HPP_

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
namespace rudy_control
{

/// Minimal `SystemInterface` that mirrors commanded position into reported state (loopback).
class RudyTopicLoopbackHardware : public hardware_interface::SystemInterface
{
public:
  RCLCPP_SHARED_PTR_DEFINITIONS(RudyTopicLoopbackHardware)

  hardware_interface::CallbackReturn on_init(const hardware_interface::HardwareInfo & info) override;

  std::vector<hardware_interface::StateInterface> export_state_interfaces() override;

  std::vector<hardware_interface::CommandInterface> export_command_interfaces() override;

  hardware_interface::return_type read(const rclcpp::Time & time, const rclcpp::Duration & period) override;

  hardware_interface::return_type write(const rclcpp::Time & time, const rclcpp::Duration & period) override;

private:
  hardware_interface::HardwareInfo info_;
  std::unordered_map<std::string, double> pos_cmd_;
  std::unordered_map<std::string, double> pos_state_;
};

}  // namespace rudy_control

#endif  // RUDY_CONTROL__RUDY_TOPIC_LOOPBACK_HARDWARE_HPP_
