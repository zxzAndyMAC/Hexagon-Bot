import { useUiStore } from '../store'
import { agentColor } from '../colors'

// 默认头像：角色名首字 + agent id 哈希定色（identicon 逻辑）。
// 自定义头像存项目 .hexagon/avatars/，经 api.agentAvatar 回 data URL。

export function Avatar({ agentId, role, size = 20 }: {
  agentId: string
  role: string
  size?: number
}) {
  const url = useUiStore((s) => s.avatars[agentId])
  const common: React.CSSProperties = {
    width: size, height: size, borderRadius: '26%', flexShrink: 0,
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    fontSize: size * 0.55, fontWeight: 560, color: '#fff',
    background: agentColor(agentId),
    overflow: 'hidden',
  }
  if (url) {
    return <img src={url} alt={role} style={{ ...common, objectFit: 'cover' }} />
  }
  return <span style={common}>{role.slice(0, 1) || '?'}</span>
}
