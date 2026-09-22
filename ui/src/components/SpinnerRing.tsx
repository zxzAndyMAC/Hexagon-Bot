// SpinnerRing 移植（beautiful-ui 票 05 TaskRows，借形 MIT slev12397/beautiful-ui）：
// 双圆运行环——底色环 + 28% dash 弧绕转。语义=在途（--warn 蓝槽，
// ADR 0056-3：琥珀是「在等你」不给它）。reduced-motion 由全局熔断冻结。
export function SpinnerRing({ size = 11 }: { size?: number }) {
  const stroke = 1.4
  const r = (size - stroke) / 2
  const c = 2 * Math.PI * r
  return (
    <svg
      className="spinner-ring"
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      aria-hidden
      style={{ flexShrink: 0, display: 'inline-block', animation: 'spin 1.1s linear infinite' }}
    >
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--border-strong)" strokeWidth={stroke} />
      <circle
        cx={size / 2} cy={size / 2} r={r} fill="none"
        stroke="var(--warn)" strokeWidth={stroke} strokeLinecap="round"
        strokeDasharray={`${c * 0.28} ${c * 0.72}`}
      />
    </svg>
  )
}
