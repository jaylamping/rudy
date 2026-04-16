- [urdf](https://wiki.ros.org/urdf)
- [XML](https://wiki.ros.org/urdf/XML)
- [joint](https://wiki.ros.org/urdf/XML/joint)

Contents

1. [<Joint> element](https://wiki.ros.org/urdf/XML/joint#A.3CJoint.3E_element)
2. [Attributes](https://wiki.ros.org/urdf/XML/joint#Attributes)
3. [Elements](https://wiki.ros.org/urdf/XML/joint#Elements)

## <Joint> element

The joint element describes the kinematics and dynamics of the joint and also specifies the [safety limits](https://wiki.ros.org/pr2_controller_manager/safety_limits) of the joint.

Here is an example of a joint element:

[Toggle line numbers](https://wiki.ros.org/urdf/XML/joint#)

```
   1  <joint name="my_joint" type="floating">
   2     <origin xyz="0 0 1" rpy="0 0 3.1416"/>
   3     <parent link="link1"/>
   4     <child link="link2"/>
   5
   6     <calibration rising="0.0"/>
   7     <dynamics damping="0.0" friction="0.0"/>
   8     <limit effort="30" velocity="1.0" lower="-2.2" upper="0.7" />
   9     <safety_controller k_velocity="10" k_position="15" soft_lower_limit="-2.0" soft_upper_limit="0.5" />
  10  </joint>
```

![joint.png](https://wiki.ros.org/urdf/XML/joint?action=AttachFile&do=get&target=joint.png)

## Attributes

The joint element has two attributes:

- **name** _(required)_


  - Specifies a unique name of the joint

**type** _(required)_

  - Specifies the type of joint, where type can be one of the following:
    - **revolute** — a hinge joint that rotates along the axis and has a limited range specified by the upper and lower limits.

    - **continuous** — a continuous hinge joint that rotates around the axis and has no upper and lower limits.

    - **prismatic** — a sliding joint that slides along the axis, and has a limited range specified by the upper and lower limits.

    - **fixed** — this is not really a joint because it cannot move. All degrees of freedom are locked. This type of joint does not require the **<axis>**, **<calibration>**, **<dynamics>**, **<limits>** or **<safety\_controller>**.

    - **floating** — this joint allows motion for all 6 degrees of freedom.

    - **planar** — this joint allows motion in a plane perpendicular to the axis.

## Elements

The joint element has following elements:

- **<origin>** _(optional: defaults to identity if not specified)_


  - This is the transform from the parent link to the child link. The joint is located at the origin of the child link, as shown in the figure above.
    **xyz** _(optional: defaults to zero vector)_


    - Represents the _x_, _y_, _z_ offset. All positions are specified in _metres_.


**rpy** _(optional: defaults to zero vector)_

    - Represents the rotation around fixed axis: first _roll_ around _x_, then _pitch_ around _y_ and finally _yaw_ around _z_. All angles are specified in _radians_.

**<parent>** _(required)_

  - Parent link name with mandatory attribute:
    **link**

    - The name of the link that is the parent of this link in the robot tree structure.

**<child>** _(required)_

  - Child link name with mandatory attribute:
    **link**

    - The name of the link that is the child link.

**<axis>** _(optional: defaults to (1,0,0))_

  - The joint axis specified in the joint frame. This is the axis of rotation for revolute joints, the axis of translation for prismatic joints, and the surface normal for planar joints. The axis is specified in the joint frame of reference. Fixed and floating joints do not use the axis field.
    **xyz** _(required)_

    - Represents the ( _x_, _y_, _z_) components of a vector. The vector should be normalized.

**<calibration>** _(optional)_

  - The reference positions of the joint, used to calibrate the absolute position of the joint.
    **rising** _(optional)_


    - When the joint moves in a positive direction, this reference position will trigger a rising edge.

**falling** _(optional)_

    - When the joint moves in a positive direction, this reference position will trigger a falling edge.

**<dynamics>** _(optional)_

  - An element specifying physical properties of the joint. These values are used to specify modeling properties of the joint, particularly useful for simulation.
    **damping** _(optional, defaults to 0)_


    - The physical damping value of the joint (in _newton-seconds per metre_ \[ _N_ ∙ _s_/ _m_\] for prismatic joints, in _newton-metre-seconds per radian_ \[ _N_ ∙ _m_ ∙ _s_/ _rad_\] for revolute joints).


**friction** _(optional, defaults to 0)_

    - The physical static friction value of the joint (in _newtons_ \[ _N_\] for prismatic joints, in _newton-metres_ \[ _N_ ∙ _m_\] for revolute joints).

**<limit>** _(required only for revolute and prismatic joint)_

  - An element can contain the following attributes:
    **lower** _(optional, defaults to 0)_


    - An attribute specifying the lower joint limit (in _radians_ for revolute joints, in _metres_ for prismatic joints). Omit if joint is continuous.


**upper** _(optional, defaults to 0)_

    - An attribute specifying the upper joint limit (in _radians_ for revolute joints, in _metres_ for prismatic joints). Omit if joint is continuous.


**effort** _(required)_

    - An attribute for enforcing the maximum joint effort (\| _applied effort_ \| < \| _effort_ \|). [See safety limits](https://wiki.ros.org/pr2_controller_manager/safety_limits).


**velocity** _(required)_

    - An attribute for enforcing the maximum joint velocity (in _radians per second_ \[ _rad_/ _s_\] for revolute joints, in _metres per second_ \[ _m_/ _s_\] for prismatic joints). [See safety limits](https://wiki.ros.org/pr2_controller_manager/safety_limits).

**<mimic>** _(optional)_ _(New with ROS Groovy. See [issue](https://github.com/ros/robot_state_publisher/issues/1))_

  - This tag is used to specify that the defined joint mimics another existing joint. The value of this joint can be computed as _value = multiplier \* other\_joint\_value + offset_.

  - **Expected and optional attributes**:

  - **joint** _(required)_

    - This specifies the name of the joint to mimic.
  - **multiplier** _(optional)_

    - Specifies the multiplicative factor in the formula above.
  - **offset** _(optional)_

    - Specifies the offset to add in the formula above. Defaults to 0 (radians for revolute joints, meters for prismatic joints)

**<safety\_controller>** _(optional)_

  - An element can contain the following attributes:
    **soft\_lower\_limit** _(optional, defaults to 0)_


    - An attribute specifying the lower joint boundary where the safety controller starts limiting the position of the joint. This limit needs to be larger than the lower joint limit (see above). See [See safety limits](https://wiki.ros.org/pr2_controller_manager/safety_limits) for more details.


**soft\_upper\_limit** _(optional, defaults to 0)_

    - An attribute specifying the upper joint boundary where the safety controller starts limiting the position of the joint. This limit needs to be smaller than the upper joint limit (see above). See [See safety limits](https://wiki.ros.org/pr2_controller_manager/safety_limits) for more details.


**k\_position** _(optional, defaults to 0)_

    - An attribute specifying the relation between position and velocity limits. See [See safety limits](https://wiki.ros.org/pr2_controller_manager/safety_limits) for more details.


**k\_velocity** _(required)_

    - An attribute specifying the relation between effort and velocity limits. See [See safety limits](https://wiki.ros.org/pr2_controller_manager/safety_limits) for more details.

Wiki: urdf/XML/joint (last edited 2022-06-17 23:41:48 by [IlyaPankov](https://wiki.ros.org/IlyaPankov "clever0ne @ 31.134.187.26[31.134.187.26]"))