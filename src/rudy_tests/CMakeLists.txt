cmake_minimum_required(VERSION 3.14)
project(rudy_tests)

find_package(ament_cmake REQUIRED)

if(BUILD_TESTING)
  find_package(ament_lint_auto REQUIRED)
  ament_lint_auto_find_test_dependencies()
  find_package(launch_testing_ament_cmake REQUIRED)

  add_launch_test(test/test_robot_state_headless.launch.py TIMEOUT 120)
endif()

ament_package()
