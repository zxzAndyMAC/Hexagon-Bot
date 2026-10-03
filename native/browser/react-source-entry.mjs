// The /core subpath exports getStack without initializing React Grab's picker.
// Only stack location fields leave this adapter; no HTML/context/clipboard API.
import { getStack } from 'react-grab/core'
export async function sourceForElement(element) {
  if (!['localhost', '127.0.0.1', '[::1]'].includes(location.hostname)) return null
  const frames = await Promise.race([getStack(element), new Promise(resolve => setTimeout(() => resolve(null), 700))])
  const source = frames?.slice(0, 12).find(frame => typeof frame.fileName === 'string' && Number.isSafeInteger(frame.lineNumber) && frame.lineNumber > 0)
  return source ? { file: source.fileName.slice(0, 400), line: source.lineNumber } : null
}
