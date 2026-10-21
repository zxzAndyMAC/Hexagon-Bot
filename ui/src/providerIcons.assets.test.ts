// owner-followups-1015 #23：随包服务商图标（Cherry Studio 9d1bf05，81 个 SVG）的完整性门。
// 升级快照 = 换 assets/providers/ 下的 SVG 与 manifest.json，这里锁住：逐字节 sha256、不引外部资源 /
// 不带脚本、清单与目录一一对应、core 目录 extras.json 里每个 iconKey 都能解析出资产。
// 资产尚未落盘时（manifest.json 不存在）整组跳过——查找接缝与兜底的行为测试在 ProviderIcon.test.tsx。
import { describe, expect, it } from 'vitest'
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { providerIconAsset } from './providerIcons'

// 不写 new URL(相对路径, import.meta.url)：Vite 会把它当资产引用改写成非 file: URL
const HERE = dirname(fileURLToPath(import.meta.url))
const DIR = join(HERE, 'assets/providers/')
const MANIFEST = `${DIR}manifest.json`
const EXTRAS = join(HERE, '../../crates/hexagon-core/data/provider-catalog/extras.json')
const SNAPSHOT_COMMIT = '9d1bf05fe2c78231d43e383118d5131f8ad5f49f'
const ASSET_COUNT = 81

type Manifest = { commit: string; assets: { file: string; upstream_path: string; sha256: string }[] }

describe.skipIf(!existsSync(MANIFEST))('随包服务商图标资产', () => {
  const manifest = () => JSON.parse(readFileSync(MANIFEST, 'utf8')) as Manifest

  it('固定快照提交、81 个文件，清单与目录里的 SVG 一一对应', () => {
    const m = manifest()
    expect(m.commit).toBe(SNAPSHOT_COMMIT)
    const listed = m.assets.map((a) => a.file).sort()
    expect(listed).toHaveLength(ASSET_COUNT)
    expect(readdirSync(DIR).filter((f) => f.endsWith('.svg')).sort()).toEqual(listed)
  })

  it('逐字节 sha256 与清单一致，SVG 不引外部资源、不带脚本', () => {
    for (const a of manifest().assets) {
      const bytes = readFileSync(`${DIR}${a.file}`)
      expect(createHash('sha256').update(bytes).digest('hex'), a.file).toBe(a.sha256)
      expect(bytes.toString(), a.file).not.toMatch(/(?:href|src)=["']https?:|<script\b/i)
    }
  })

  it('上游 LICENSE 随包', () => {
    expect(existsSync(`${DIR}UPSTREAM-REGISTRY-LICENSE`)).toBe(true)
    expect(existsSync(`${DIR}UPSTREAM-ROOT-LICENSE`)).toBe(true)
  })

  it('core 目录 extras.json 的每个 iconKey 都解析得到资产', () => {
    const extras = JSON.parse(readFileSync(EXTRAS, 'utf8')) as { providers: Record<string, { iconKey: string }> }
    const missing = Object.entries(extras.providers)
      .filter(([, e]) => e.iconKey && !providerIconAsset(e.iconKey))
      .map(([id, e]) => `${id}:${e.iconKey}`)
    expect(missing).toEqual([])
  })
})
