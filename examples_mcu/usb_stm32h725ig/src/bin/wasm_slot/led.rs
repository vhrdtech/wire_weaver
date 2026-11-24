//! The B135B status LED: RGB D201, common anode, each colour lit while its pin is low. It breathes through
//! bedrock_led on PWM, not as a plain GPIO at full brightness.
//!
//! | Colour | Pin  | Output                     |
//! |--------|------|----------------------------|
//! | red    | PC7  | TIM3_CH2 (AF2)             |
//! | green  | PC6  | TIM3_CH1 (AF2)             |
//! | blue   | PH13 | TIM8_CH1N (AF3), CH1 unused |
//!
//! PC6 and PC7 are on TIM8_CH1/CH2 too, but blue's PH13 only has TIM8_CH1N: green on TIM8_CH1 would share blue's
//! compare value, so green and red take TIM3.
//!
//! ES0491 Rev 9, 2.14.2 "Consecutive compare event missed in specific conditions" (TIM, not fixed on rev Z or Y): a
//! compare event is missed when CCR goes from ARR to 0 in two consecutive counter cycles. In PWM mode that is one
//! period at the old duty, invisible on a LED; no workaround here.

use bedrock_led::{Channel, Indication, Pwm, look};
use embassy_stm32::peripherals::{TIM3, TIM8};
use embassy_stm32::timer::Channel as TimChannel;
use embassy_stm32::timer::complementary_pwm::ComplementaryPwm;
use embassy_stm32::timer::simple_pwm::SimplePwmChannel;

/// The one RGB LED, index 0.
pub static LEDS: Indication<1> = Indication::new();

/// Blue on TIM8_CH1N. With PWM mode 1 and no dead time CH1N is the inverse of OC1REF, so it is low for the compare
/// value: the duty lights the LED as it is, no inversion needed.
pub struct Blue {
    pwm: ComplementaryPwm<'static, TIM8>,
    max: u32,
}

impl Blue {
    pub fn new(mut pwm: ComplementaryPwm<'static, TIM8>) -> Self {
        let max = pwm.get_max_duty();
        pwm.set_duty(TimChannel::Ch1, 0);
        pwm.enable(TimChannel::Ch1);
        Self { pwm, max }
    }
}

impl Channel for Blue {
    fn max_duty(&self) -> u32 {
        self.max
    }

    fn set_duty(&mut self, duty: u32) {
        // set_duty asserts duty <= max: clamp, a service task never panics
        self.pwm.set_duty(TimChannel::Ch1, duty.min(self.max));
    }
}

#[embassy_executor::task]
pub async fn led_task(
    mut red: Pwm<SimplePwmChannel<'static, TIM3>>,
    mut green: Pwm<SimplePwmChannel<'static, TIM3>>,
    mut blue: Blue,
) {
    LEDS.set_colour(0, look::OK.colour);
    LEDS.run([look::OK.mode], |leds| {
        let [r, g, b] = leds.rgb(0);
        red.set_level(r);
        green.set_level(g);
        blue.set_level(b);
    })
    .await
}
