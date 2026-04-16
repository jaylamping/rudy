cmake_minimum_required(VERSION 3.14)
project(driver)

find_package(ament_cmake REQUIRED)

find_program(CARGO cargo REQUIRED)

set(_driver_cargo_target_dir "${CMAKE_CURRENT_BINARY_DIR}/cargo_target")
set(_driver_bin "${_driver_cargo_target_dir}/release/driver_node")

add_custom_command(
  OUTPUT "${_driver_bin}"
  COMMAND ${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${_driver_cargo_target_dir}
    ${CARGO} build --release --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
  WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
  COMMENT "Building driver (Rust)"
  VERBATIM
)

add_custom_target(driver_rust ALL DEPENDS "${_driver_bin}")

install(PROGRAMS "${_driver_bin}" DESTINATION lib/${PROJECT_NAME})

if(BUILD_TESTING)
  find_package(ament_lint_auto REQUIRED)
  ament_lint_auto_find_test_dependencies()
endif()

ament_package()
