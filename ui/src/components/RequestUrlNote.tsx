// 票 21：Base URL 下方「实际请求地址」的 help 内容（地址由 requestUrl.ts 的 useRequestUrl 问 core）。
import { useTranslation } from 'react-i18next'

/** Field 的 help 内容：「实际请求地址：<等宽地址>」。调用方只在 url 非空时传给 Field.help
 * （Field 见到任何元素都会渲染 help 行）。标签自带各语言的冒号与空格。 */
export function RequestUrlNote({ url }: { url: string }) {
  const { t } = useTranslation()
  return (
    <span data-request-url>
      {t('providers.requestUrl')}<span className="font-mono break-all">{url}</span>
    </span>
  )
}
