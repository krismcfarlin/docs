<script lang="ts">
  import { onMount } from 'svelte';
  import * as d3 from 'd3';
  import { getGraphData, type GraphNode, type GraphEdge, type GraphData } from '$lib/api';

  interface Props {
    space_id: string;
    onselect?: (pageId: string) => void;
  }

  let { space_id, onselect }: Props = $props();

  let svgEl = $state<SVGSVGElement | undefined>(undefined);
  let containerEl = $state<HTMLDivElement | undefined>(undefined);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let graphData = $state<GraphData | null>(null);

  // Tooltip state
  let tooltip = $state<{ visible: boolean; x: number; y: number; label: string; detail: string }>({
    visible: false, x: 0, y: 0, label: '', detail: '',
  });

  function nodeColor(node: GraphNode): string {
    if (node.node_type === 'page') return '#3b82f6';
    switch (node.entity_type) {
      case 'person':   return '#f59e0b';
      case 'project':  return '#10b981';
      case 'concept':  return '#8b5cf6';
      case 'decision': return '#f43f5e';
      default:         return '#8b5cf6';
    }
  }

  function nodeOpacity(node: GraphNode): number {
    return node.status === 'candidate' ? 0.5 : 1;
  }

  function nodeRadius(node: GraphNode): number {
    if (node.node_type === 'page') return 10;
    const count = node.mention_count ?? 1;
    return Math.max(6, Math.min(16, 6 + (count - 1) * 1.5));
  }

  function buildGraph(data: GraphData): void {
    if (!svgEl || !containerEl) return;

    const width = containerEl.clientWidth;
    const height = containerEl.clientHeight;

    // Clear previous render
    d3.select(svgEl).selectAll('*').remove();

    const svg = d3.select(svgEl)
      .attr('width', width)
      .attr('height', height);

    // Root group that zoom/pan will transform
    const g = svg.append('g');

    // Zoom behaviour
    const zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.1, 4])
      .on('zoom', (event) => {
        g.attr('transform', event.transform);
      });

    svg.call(zoom);

    // Defs — dashed pattern for mention edges
    const defs = svg.append('defs');
    defs.append('marker')
      .attr('id', 'arrow-link')
      .attr('viewBox', '0 -4 8 8')
      .attr('refX', 18)
      .attr('refY', 0)
      .attr('markerWidth', 6)
      .attr('markerHeight', 6)
      .attr('orient', 'auto')
      .append('path')
      .attr('d', 'M0,-4L8,0L0,4')
      .attr('fill', 'rgba(148,163,184,0.4)');

    // Prepare simulation nodes/links — d3 mutates these objects
    type SimNode = GraphNode & d3.SimulationNodeDatum;
    type SimLink = { source: string | SimNode; target: string | SimNode; edge_type: string; label?: string };

    const simNodes: SimNode[] = data.nodes.map(n => ({ ...n }));
    const simLinks: SimLink[] = data.edges.map(e => ({ ...e }));

    const simulation = d3.forceSimulation<SimNode>(simNodes)
      .force('link', d3.forceLink<SimNode, SimLink>(simLinks)
        .id(d => d.id)
        .distance(d => {
          const s = d.source as SimNode;
          const t = d.target as SimNode;
          if (s.node_type === 'page' && t.node_type === 'page') return 120;
          return 80;
        })
        .strength(0.4))
      .force('charge', d3.forceManyBody().strength(-180))
      .force('center', d3.forceCenter(width / 2, height / 2))
      .force('collision', d3.forceCollide<SimNode>().radius(d => nodeRadius(d) + 8));

    // Edges
    const link = g.append('g')
      .attr('class', 'links')
      .selectAll<SVGLineElement, SimLink>('line')
      .data(simLinks)
      .join('line')
      .attr('stroke', 'rgba(148,163,184,0.3)')
      .attr('stroke-width', 1.2)
      .attr('stroke-dasharray', d => d.edge_type === 'mention' ? '4 3' : 'none')
      .attr('marker-end', d => d.edge_type === 'link' ? 'url(#arrow-link)' : null);

    // Node groups
    const node = g.append('g')
      .attr('class', 'nodes')
      .selectAll<SVGGElement, SimNode>('g')
      .data(simNodes)
      .join('g')
      .attr('cursor', d => d.node_type === 'page' ? 'pointer' : 'default')
      .call(
        d3.drag<SVGGElement, SimNode>()
          .on('start', (event, d) => {
            if (!event.active) simulation.alphaTarget(0.3).restart();
            d.fx = d.x;
            d.fy = d.y;
          })
          .on('drag', (event, d) => {
            d.fx = event.x;
            d.fy = event.y;
          })
          .on('end', (event, d) => {
            if (!event.active) simulation.alphaTarget(0);
            d.fx = null;
            d.fy = null;
          })
      );

    // Circles
    node.append('circle')
      .attr('r', d => nodeRadius(d))
      .attr('fill', d => nodeColor(d))
      .attr('fill-opacity', d => nodeOpacity(d))
      .attr('stroke', d => nodeColor(d))
      .attr('stroke-width', 1.5)
      .attr('stroke-opacity', d => nodeOpacity(d) * 0.6);

    // Labels for promoted entities and pages (always visible)
    node.filter(d => d.node_type === 'page' || d.status === 'promoted')
      .append('text')
      .attr('dy', d => nodeRadius(d) + 11)
      .attr('text-anchor', 'middle')
      .attr('fill', 'rgba(203,213,225,0.85)')
      .attr('font-size', '9px')
      .attr('pointer-events', 'none')
      .text(d => d.label.length > 22 ? d.label.slice(0, 20) + '…' : d.label);

    // Hover interactions
    node
      .on('mouseover', function(event: MouseEvent, d: SimNode) {
        d3.select(this).select('circle')
          .attr('stroke-width', 3)
          .attr('stroke-opacity', 1);

        const detail = d.node_type === 'entity'
          ? `${d.entity_type ?? 'concept'} · ${d.mention_count ?? 0} mention${(d.mention_count ?? 0) !== 1 ? 's' : ''}`
          : 'page';

        const rect = (svgEl as SVGSVGElement).getBoundingClientRect();
        tooltip = {
          visible: true,
          x: event.clientX - rect.left + 12,
          y: event.clientY - rect.top - 8,
          label: d.label,
          detail,
        };
      })
      .on('mousemove', function(event: MouseEvent) {
        const rect = (svgEl as SVGSVGElement).getBoundingClientRect();
        tooltip = {
          ...tooltip,
          x: event.clientX - rect.left + 12,
          y: event.clientY - rect.top - 8,
        };
      })
      .on('mouseout', function(_event: MouseEvent, d: SimNode) {
        d3.select(this).select('circle')
          .attr('stroke-width', 1.5)
          .attr('stroke-opacity', nodeOpacity(d) * 0.6);
        tooltip = { ...tooltip, visible: false };
      })
      .on('click', function(_event: MouseEvent, d: SimNode) {
        if (d.node_type === 'page' && onselect) {
          onselect(d.id);
        }
      });

    // Tick
    simulation.on('tick', () => {
      link
        .attr('x1', d => (d.source as SimNode).x ?? 0)
        .attr('y1', d => (d.source as SimNode).y ?? 0)
        .attr('x2', d => (d.target as SimNode).x ?? 0)
        .attr('y2', d => (d.target as SimNode).y ?? 0);

      node.attr('transform', d => `translate(${d.x ?? 0},${d.y ?? 0})`);
    });
  }

  onMount(async () => {
    try {
      const data = await getGraphData(space_id);
      graphData = data;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  $effect(() => {
    if (!loading && graphData && svgEl && containerEl) {
      buildGraph(graphData);
    }
  });
</script>

<div class="w-full h-full relative" bind:this={containerEl}>
  {#if loading}
    <div class="absolute inset-0 flex items-center justify-center">
      <div class="flex flex-col items-center gap-3 text-slate-400">
        <svg class="w-8 h-8 animate-spin" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z"></path>
        </svg>
        <span class="text-sm">Loading graph…</span>
      </div>
    </div>
  {:else if error}
    <div class="absolute inset-0 flex items-center justify-center">
      <p class="text-rose-400 text-sm">Error loading graph: {error}</p>
    </div>
  {:else if !graphData || graphData.nodes.length === 0}
    <div class="absolute inset-0 flex items-center justify-center">
      <div class="text-center text-slate-500 max-w-xs">
        <svg class="w-12 h-12 mx-auto mb-3 opacity-30" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"
            d="M9 20l-5.447-2.724A1 1 0 013 16.382V5.618a1 1 0 011.447-.894L9 7m0 13l6-3m-6 3V7m6 10l4.553 2.276A1 1 0 0021 18.382V7.618a1 1 0 00-.553-.894L15 4m0 13V4m0 0L9 7" />
        </svg>
        <p class="text-sm font-medium text-slate-400 mb-1">No graph data yet</p>
        <p class="text-xs">Synthesize some pages first to build the knowledge graph.</p>
      </div>
    </div>
  {:else}
    <svg bind:this={svgEl} class="w-full h-full" style="background: transparent;"></svg>

    <!-- Tooltip -->
    {#if tooltip.visible}
      <div
        class="absolute pointer-events-none z-10 px-2.5 py-1.5 rounded-lg text-xs shadow-xl border border-slate-700"
        style="left: {tooltip.x}px; top: {tooltip.y}px; background: #1e293b; max-width: 200px;"
      >
        <div class="font-semibold text-slate-100 truncate">{tooltip.label}</div>
        <div class="text-slate-400 mt-0.5">{tooltip.detail}</div>
      </div>
    {/if}

    <!-- Legend -->
    <div class="absolute bottom-4 left-4 flex flex-col gap-1.5 p-3 rounded-xl border border-slate-700/60"
      style="background: rgba(15,23,42,0.85); backdrop-filter: blur(8px);">
      <p class="text-xs text-slate-500 font-medium mb-1 uppercase tracking-wider">Legend</p>
      {#each [
        { color: '#3b82f6', label: 'Page' },
        { color: '#f59e0b', label: 'Person' },
        { color: '#10b981', label: 'Project' },
        { color: '#8b5cf6', label: 'Concept' },
        { color: '#f43f5e', label: 'Decision' },
      ] as item}
        <div class="flex items-center gap-2">
          <span class="w-2.5 h-2.5 rounded-full flex-shrink-0" style="background: {item.color};"></span>
          <span class="text-xs text-slate-400">{item.label}</span>
        </div>
      {/each}
      <div class="flex items-center gap-2 mt-1">
        <svg width="24" height="8" class="flex-shrink-0">
          <line x1="0" y1="4" x2="24" y2="4" stroke="rgba(148,163,184,0.5)" stroke-width="1.5" stroke-dasharray="4 3"/>
        </svg>
        <span class="text-xs text-slate-400">Mention</span>
      </div>
      <div class="flex items-center gap-2">
        <svg width="24" height="8" class="flex-shrink-0">
          <line x1="0" y1="4" x2="24" y2="4" stroke="rgba(148,163,184,0.5)" stroke-width="1.5"/>
        </svg>
        <span class="text-xs text-slate-400">Link</span>
      </div>
    </div>

    <!-- Node count badge -->
    <div class="absolute top-4 right-4 px-3 py-1.5 rounded-lg text-xs text-slate-400 border border-slate-700/60"
      style="background: rgba(15,23,42,0.85); backdrop-filter: blur(8px);">
      {graphData.nodes.filter(n => n.node_type === 'page').length} pages ·
      {graphData.nodes.filter(n => n.node_type === 'entity').length} entities ·
      {graphData.edges.length} edges
    </div>
  {/if}
</div>
