import { beforeEach, expect, it, vi } from 'vitest'
const mocked = vi.hoisted(() => ({ status: vi.fn(), control: vi.fn(), toast: vi.fn() }))
vi.mock('./api', () => ({ api: { desktopStatus: mocked.status, desktopControl: mocked.control }, errText: String }))
vi.mock('./store', () => ({ useUiStore: { getState: () => ({ pushToast: mocked.toast }) } }))
import { resumeDesktop } from './desktopPause'
beforeEach(() => vi.clearAllMocks())
it('resumes using the fresh host project after the preview has disappeared', async () => {
  mocked.status.mockResolvedValue({ project_root: '/current/project', paused: true })
  mocked.control.mockResolvedValue({ paused: false })
  await resumeDesktop()
  expect(mocked.control).toHaveBeenCalledWith('resume', '/current/project')
})
it('does not resume another project when the host rejects the bound root', async () => {
  mocked.status.mockResolvedValue({ project_root: '/old/project', paused: true })
  mocked.control.mockRejectedValue(new Error('project changed'))
  await resumeDesktop()
  expect(mocked.control).toHaveBeenCalledTimes(1)
  expect(mocked.toast).toHaveBeenCalledWith('Error: project changed', 'err')
})
