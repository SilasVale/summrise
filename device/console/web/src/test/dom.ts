/**
 * Put a replaced global back exactly as it was, or remove the property when jsdom never had it.
 *
 * A test that stubs `URL.createObjectURL`, `navigator.clipboard` or `matchMedia` takes the original
 * descriptor first and restores it afterwards. That gesture is the same in every file, so it lives
 * here once instead of as a private copy per suite.
 */
export function restoreGlobal(target: object, key: string, descriptor?: PropertyDescriptor): void {
  if (descriptor) Object.defineProperty(target, key, descriptor);
  else Reflect.deleteProperty(target, key);
}
