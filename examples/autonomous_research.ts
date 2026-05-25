// examples/autonomous_research.ts
// Run: bua agent run examples/autonomous_research.ts \
//        --allow-net=* --allow-fs=./output --allow-env

import * as trace from 'bua:trace';
import * as fs from 'bua:fs';
import { call } from 'bua:tools';
import { spawn, spawnAll } from 'bua:agent';
import { checkpoint } from 'bua:trace';

interface ResearchTask {
  query: string;
  depth: 'shallow' | 'deep';
  outputFile: string;
}

interface ResearchResult {
  query: string;
  findings: string[];
  sources: string[];
  confidence: number;
}

// ---------------------------------------------------------------------------
// Research orchestrator — supervisor agent
// ---------------------------------------------------------------------------

async function research(task: ResearchTask): Promise<ResearchResult> {
  trace.info(`Starting research: "${task.query}"`);

  // Phase 1: Initial search
  await checkpoint('pre-search');
  const searchResult = await call('bua_http_get', {
    url: `https://en.wikipedia.org/api/rest_v1/page/summary/${encodeURIComponent(task.query)}`,
    headers: { 'User-Agent': 'Bua-Research-Agent/0.1' },
  });

  const summary = searchResult as { status: number; body: string };

  if (summary.status !== 200) {
    trace.warn(`Search returned status ${summary.status}`);
  }

  let findings: string[] = [];
  let sources: string[] = [];

  try {
    const data = JSON.parse(summary.body);
    if (data.extract) {
      findings.push(data.extract);
      sources.push(data.content_urls?.desktop?.page ?? 'Wikipedia');
    }
  } catch {
    trace.warn('Failed to parse search result');
  }

  await checkpoint('post-search');

  // Phase 2: Deep research via parallel sub-agents (if requested)
  if (task.depth === 'deep' && findings.length > 0) {
    trace.info('Spawning parallel research workers');

    const subtopics = extractSubtopics(findings[0] ?? '');

    const workerResults = await Promise.allSettled(
      subtopics.slice(0, 3).map(subtopic =>
        spawn({
          entrypoint: './examples/research_worker.ts',
          allowNet: ['en.wikipedia.org'],
          timeout: 15_000,
        })
      )
    );

    for (const result of workerResults) {
      if (result.status === 'fulfilled' && result.value) {
        findings.push(`Sub-research: ${JSON.stringify(result.value)}`);
      }
    }
  }

  const output: ResearchResult = {
    query: task.query,
    findings,
    sources,
    confidence: findings.length > 0 ? 0.85 : 0.2,
  };

  // Write results
  const outputPath = `./output/${task.outputFile}`;
  await fs.writeFile(outputPath, JSON.stringify(output, null, 2));
  trace.info(`Results written to ${outputPath}`);

  await checkpoint('complete');
  return output;
}

function extractSubtopics(text: string): string[] {
  // Simple heuristic: extract capitalized multi-word phrases
  const matches = text.match(/[A-Z][a-z]+ [A-Z][a-z]+/g) ?? [];
  return [...new Set(matches)].slice(0, 5);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

const task: ResearchTask = {
  query: Bua.env.get('RESEARCH_QUERY') ?? 'artificial intelligence',
  depth: (Bua.env.get('RESEARCH_DEPTH') as 'shallow' | 'deep') ?? 'shallow',
  outputFile: `research-${Date.now()}.json`,
};

research(task)
  .then(result => {
    trace.info(`Research complete. Confidence: ${result.confidence}`);
    trace.info(`Findings: ${result.findings.length}`);
    console.log(JSON.stringify(result, null, 2));
  })
  .catch(err => {
    trace.error(`Research failed: ${err.message}`);
    process.exit(1);
  });
