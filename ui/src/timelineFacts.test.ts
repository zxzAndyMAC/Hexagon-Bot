import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { api } from './api'
import { useUiStore } from './store'
import { deriveWorkbenchStatusFromFacts } from './timelineFacts'
import type { TimelineFacts } from './gen/TimelineFacts'

const facts = (latest_event_id = 100): TimelineFacts => ({ project_root: '/project', latest_event_id,
  active_turns: [{ agent_id: 'worker', start_event_id: 20, started_at: '2026-10-02T00:00:00Z', current_tool: 'bash' }],
  latest_contexts: [], latest_agent_messages: [], latest_plans: [], settled_tool_streams: [], approval_mode_revision: 1,
  latest_turn_start_id: 20, steered_message_ids: [] })
beforeEach(() => {
  useUiStore.getState().beginProjectSwitch()
  useUiStore.getState().commitProjectRoot('/project', useUiStore.getState().projectEpoch)
})
afterEach(() => vi.restoreAllMocks())
it('shows current activity even when no history window is loaded', () => {
  expect(deriveWorkbenchStatusFromFacts({ facts: facts(), team: [{id:'worker',role:'QA'}], stages:[], streams:{},thinkings:{},streamDone:{} }))
    .toEqual({stage:null,role:'QA',tool:'bash'})
})
it('clears only explicitly settled watched tool buffers, preserving concurrent work', async () => {
  useUiStore.getState().applyToolOutput({agent_id:'worker',seq:'["req:1","call:1"]',stream:'stdout',text:'done'})
  useUiStore.getState().applyToolOutput({agent_id:'worker',seq:'["req:1","call:2"]',stream:'stdout',text:'running'})
  const response=facts();response.settled_tool_streams=[{agent_id:'worker',seq:'["req:1","call:1"]'}]
  const read=vi.spyOn(api,'timelineFacts').mockResolvedValue(response)
  await useUiStore.getState().refreshFacts()
  expect(read.mock.calls[0][0].watched_tool_streams).toHaveLength(2)
  expect(Object.values(useUiStore.getState().toolStreams)).toEqual(['running'])
})
it('cannot restore old-project facts after switching to another project', async () => {
  let resolve!:(v:TimelineFacts)=>void
  vi.spyOn(api,'timelineFacts').mockImplementation(()=>new Promise(yes=>{resolve=yes}))
  const pending=useUiStore.getState().refreshFacts()
  useUiStore.getState().beginProjectSwitch()
  useUiStore.getState().commitProjectRoot('/other',useUiStore.getState().projectEpoch)
  resolve(facts());await pending
  expect(useUiStore.getState().timelineFacts).toBeNull()
})
it('facts failure does not suppress pending cards or stage refresh',async()=>{
  vi.spyOn(api,'timelineFacts').mockRejectedValue(new Error('facts unavailable'))
  vi.spyOn(api,'stageStatus').mockResolvedValue([])
  vi.spyOn(api,'pendingQuestions').mockResolvedValue([{id:'q-new',kind:'permission',agent_id:null,payload:{},state:'queued'}])
  vi.spyOn(api,'intakeDraftPending').mockResolvedValue(false)
  await useUiStore.getState().refreshFast()
  expect(useUiStore.getState().pending.map(item=>item.id)).toEqual(['q-new'])
  expect(useUiStore.getState().factsError).toContain('facts unavailable')
})
