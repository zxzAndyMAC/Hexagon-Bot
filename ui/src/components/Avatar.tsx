import { useUiStore } from '../store'

// 默认头像：角色名首字 + agent id 哈希定色（identicon 逻辑）。
// 自定义头像存项目 .hexagon/avatars/，经 api.agentAvatar 回 data URL。

function hashHue(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0
  return h % 360
}

export function Avatar({ agentId, role, size = 20, square = false }: {
  agentId: string
  role: string
  size?: number
  square?: boolean
}) {
  const url = useUiStore((s) => s.avatars[agentId])
  const common: React.CSSProperties = {
    width: size, height: size, borderRadius: square ? '26%' : '50%', flexShrink: 0,
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    fontSize: size * 0.55, fontWeight: 560, color: '#fff',
    background: `hsl(${hashHue(agentId)}, 45%, 42%)`,
    overflow: 'hidden',
  }
  if (url) {
    return <img src={url} alt={role} style={{ ...common, objectFit: 'cover' }} />
  }
  return <span style={common}>{role.slice(0, 1) || '?'}</span>
}
