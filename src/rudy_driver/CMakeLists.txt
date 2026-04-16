cmake_minimum_required(VERSION 3.14)
project(rudy_driver)

find_package(ament_cmake REQUIRED)

find_program(CARGO cargo REQUIRED)

set(_rudy_cargo_target_dir "${CMAKE_CURRENT_BINARY_DIR}/cargo_target")
set(_rudy_bin "${_rudy_cargo_target_dir}/release/rudy_driver_node")

add_custom_command(
  OUTPUT "${_rudy_bin}"
  COMMAND ${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${_rudy_cargo_target_dir}
    ${CARGO} build --release --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
  WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
  COMMENT "Building rudy_driver (Rust)"
  VERBATIM
)

add_custom_target(rudy_driver_rust ALL DEPENDS "${_rudy_bin}")

install(PROGRAMS "${_rudy_bin}" DESTINATION lib/${PROJECT_NAME})

if(BUILD_TESTING)
  find_package(ament_lint_auto REQUIRED)
  ament_lint_auto_find_test_dependencies()
endif()

ament_package()
