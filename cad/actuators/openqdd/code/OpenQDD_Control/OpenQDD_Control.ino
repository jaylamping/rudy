// Libraries
#include <HardwareSerial.h>
#include <SoftwareSerial.h>
#include <ODriveArduino.h>
// Printing with stream operator helper functions
template<class T> inline Print& operator<<(Print& obj, T arg) {
  obj.print(arg);
  return obj;
}
template<> inline Print& operator<<(Print& obj, float arg) {
  obj.print(arg, 4);
  return obj;
}


// **ODrive Serial Configurations**
// --------------------------------
// Teensy 3 and 4 (all versions) - Serial1
// pin 0: RX - connect to ODrive TX
// pin 1: TX - connect to ODrive RX
//HardwareSerial& odrive_serial = Serial1;

// Arduino without spare serial ports (such as Arduino UNO) have to use software serial.
// Note that this is implemented poorly and can lead to wrong data sent or read.
//pin 8 : RX - connect to ODrive TX
//pin 9 : TX - connect to ODrive RX
SoftwareSerial odrive_serial(8, 9);

// ODrive object
ODriveArduino odrive(odrive_serial);

void setup() {
  // ODrive uses 115200 baud
  odrive_serial.begin(115200);

  // Serial to PC
  Serial.begin(115200);
  while (!Serial)
    ;  // wait for Arduino Serial Monitor to open

  //Setting Velocity and Current Limits
  odrive.SetVelocityLimit(0, 21);  //velocity limit (turns/sec)
  odrive.SetCurrentLimit(0, 36);   //current limit (Amps)
}

void loop() {

  if (Serial.available()) {
    char c = Serial.read();
    // Run Calibration Sequence (type "1" in the Serial Monitor)
    if (c == '1') {
      int requested_state;
      /*
      //Motor Calibration
      requested_state = AXIS_STATE_MOTOR_CALIBRATION;
      if (!odrive.run_state(0, requested_state, true)) return;
      //Encoder Offset Calibration
      requested_state = AXIS_STATE_ENCODER_OFFSET_CALIBRATION;
      if (!odrive.run_state(0, requested_state, true, 25.0f)) return;
      */

      //Start Closed Loop Control
      requested_state = AXIS_STATE_CLOSED_LOOP_CONTROL;
      if (!odrive.run_state(0, requested_state, false /*don't wait*/)) return;
    }
    if (c == 'a') {
      odrive.SetPositionGain(0, 60);
    }
  }
  Serial.println(odrive.GetPosition(0), 5);
}
