/**
 * Jalali (Persian) calendar for the island clock.
 *
 * Conversion is done by `jalaali-js` (MIT, zero dependencies) on the
 * frontend — no backend work. The island clock always shows both
 * dates: Gregorian (Latin, `en-US` formatting) and Jalali (Persian
 * month/weekday names, Persian digits).
 */
import { toJalaali } from "jalaali-js";

const PERSIAN_DIGITS = ["۰", "۱", "۲", "۳", "۴", "۵", "۶", "۷", "۸", "۹"];

/** Latin 0-9 → Persian ۰-۹ (other characters untouched). */
export function toPersianDigits(text: string): string {
  return text.replace(/[0-9]/g, (digit) => PERSIAN_DIGITS[Number(digit)]);
}

const JALALI_MONTHS = [
  "فروردین",
  "اردیبهشت",
  "خرداد",
  "تیر",
  "مرداد",
  "شهریور",
  "مهر",
  "آبان",
  "آذر",
  "دی",
  "بهمن",
  "اسفند",
];

// Sunday-first, matching Date.getDay().
const JALALI_WEEKDAYS = [
  "یکشنبه",
  "دوشنبه",
  "سه‌شنبه",
  "چهارشنبه",
  "پنجشنبه",
  "جمعه",
  "شنبه",
];

/**
 * The Jalali date for a Gregorian date.
 * `format` follows the Settings date format: "short" is day + month,
 * "full" adds the weekday and the year — the same rule as the
 * Gregorian side, so both halves of the clock always agree.
 */
export function jalaliDate(date: Date, format: string): string {
  const { jy, jm, jd } = toJalaali(
    date.getFullYear(),
    date.getMonth() + 1,
    date.getDate(),
  );
  const day = toPersianDigits(String(jd));
  const month = JALALI_MONTHS[jm - 1];
  if (format === "full") {
    const weekday = JALALI_WEEKDAYS[date.getDay()];
    const year = toPersianDigits(String(jy));
    return `${weekday}، ${day} ${month} ${year}`;
  }
  return `${day} ${month}`;
}
