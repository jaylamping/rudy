cmake_minimum_required(VERSION 3.14)
project(rudy_control)

set(CMAKE_CXX_STANDARD 17)

find_package(ament_cmake REQUIRED)
find_package(hardware_interface REQUIRED)
find_package(pluginlib REQUIRED)
find_package(rclcpp REQUIRED)

add_library(rudy_topic_loopback_hardware SHARED
  src/rudy_topic_loopback_hardware.cpp
)
target_include_directories(rudy_topic_loopback_hardware PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
ament_target_dependencies(rudy_topic_loopback_hardware
  hardware_interface
  pluginlib
  rclcpp
)
pluginlib_export_plugin_description_file(hardware_interface rudy_control_plugins.xml)

install(TARGETS rudy_topic_loopback_hardware
  DESTINATION lib
)
install(DIRECTORY config DESTINATION share/${PROJECT_NAME})

if(BUILD_TESTING)
  find_package(ament_lint_auto REQUIRED)
  ament_lint_auto_find_test_dependencies()
  find_package(ament_cmake_gtest REQUIRED)
  ament_add_gtest(test_loopback_smoke test/test_loopback_smoke.cpp)
  target_include_directories(test_loopback_smoke PRIVATE include)
  target_link_libraries(test_loopback_smoke rudy_topic_loopback_hardware)
  ament_target_dependencies(test_loopback_smoke hardware_interface rclcpp)
endif()

ament_package()
