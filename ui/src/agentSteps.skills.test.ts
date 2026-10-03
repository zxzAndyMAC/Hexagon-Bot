import { describe, expect, it } from 'vitest'
import { toolInputSummary, TOOL_LABEL } from './agentSteps'

describe('actual skill tool calls', () => {
  it('shows the selected skill name and search phrase without JSON noise', () => {
    expect(toolInputSummary({ tool: 'load_skill', input: { name: 'ui-ux-pro-max' } })).toBe('ui-ux-pro-max')
    expect(toolInputSummary({ tool: 'search_skills', input: { query: 'web design' } })).toBe('web design')
    expect(TOOL_LABEL.load_skill).toBe('stepLoadSkill')
    expect(TOOL_LABEL.search_skills).toBe('stepSearchSkills')
  })
})
