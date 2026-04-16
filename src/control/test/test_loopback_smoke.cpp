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
