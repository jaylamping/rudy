from glob import glob

from setuptools import find_namespace_packages, setup

package_name = "simulation"

setup(
    name=package_name,
    version="0.1.0",
    packages=find_namespace_packages(include=["simulation*"]),
    data_files=[
        ("share/ament_index/resource_index/packages", ["resource/" + package_name]),
        ("share/" + package_name, ["package.xml"]),
        ("share/" + package_name + "/configs", glob("configs/*.yaml")),
        ("share/" + package_name + "/launch", glob("launch/*.xml")),
    ],
    install_requires=["setuptools", "pyyaml"],
    zip_safe=True,
    maintainer="Rudy maintainers",
    maintainer_email="jlamping@users.noreply.github.com",
    description="Isaac Lab integration scaffold for Rudy.",
    license="Apache-2.0",
    tests_require=["pytest"],
    entry_points={
        "console_scripts": [
            "sim_train = simulation.scripts.train:main",
        ],
    },
)
