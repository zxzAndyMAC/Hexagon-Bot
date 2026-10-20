// 票 21（owner-followups-1015 第 12 项，2026-10-18 负责人裁决）：服务商表单 Base URL 下方的
// 「实际请求地址」。规则只在 core `provider_url` 一处（保存 / 检测同一道规范化），界面每次输入
// 问一次 core，不在 TS 里复刻——曾有一份 TS 推导（ProviderManager previewUrl）与 core 各说各话。
// 序号守卫：更晚的输入作废更早的回应。遮蔽的已存地址与空串 core 回 null，不显示预览行。
import { useEffect, useRef, useState } from 'react'
import { api, type ProviderView } from './api'

export function useRequestUrl(kind: ProviderView['kind'], baseUrl: string): string | null {
  const [state, setState] = useState<{ key: string; url: string | null }>({ key: '', url: null })
  const seq = useRef(0)
  const key = `${kind}\n${baseUrl}`
  useEffect(() => {
    const mine = ++seq.current
    if (!baseUrl.trim()) return
    api.providerRequestUrl(kind, baseUrl)
      .then((url) => { if (mine === seq.current) setState({ key, url }) })
      // 预览只是提示：命令失败就不显示，保存时 core 照常校验
      .catch(() => { if (mine === seq.current) setState({ key, url: null }) })
  }, [key, kind, baseUrl])
  return state.key === key ? state.url : null
}
