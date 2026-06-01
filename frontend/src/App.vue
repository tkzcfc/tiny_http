<script>
import { init, use } from 'echarts/core'
import { BarChart, LineChart, PieChart } from 'echarts/charts'
import { DataZoomComponent, GridComponent, LegendComponent, TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'

use([BarChart, LineChart, PieChart, DataZoomComponent, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer])

const api = async (url, options = {}) => {
  const response = await fetch(url, {
    credentials: 'include',
    headers: { 'content-type': 'application/json', ...(options.headers || {}) },
    ...options,
  })
  if (!response.ok) throw new Error(await response.text())
  return response.json()
}

export default {
  data() {
    return {
      me: { authenticated: false, is_admin: false },
      loginForm: { username: '', password: '' },
      active: 'logs',
      loading: false,
      error: '',
      logType: '',
      logTypes: [],
      logTypeForm: { log_type: '', display_name: '', enabled: true },
      page: 1,
      pageSize: 20,
      logs: { items: [], total: 0, pending: 0, solved: 0, total_pages: 0 },
      sortBy: 'last_time',
      sortOrder: 'desc',
      statusFilter: 'all',
      selectedLog: null,
      userLog: null,
      statsMode: 'errors',
      statsLogType: '',
      statsLogTypeInitialized: false,
      errorStatsView: 'overview',
      errorSourceKind: 'package',
      clientStatsView: 'overview',
      clientBreakdownKind: 'region',
      statsClientType: '',
      clientTypes: [],
      granularity: 'day',
      statsRange: '30d',
      stats: null,
      statsLoading: false,
      statsSourceLoading: false,
      statsRequestSeq: 0,
      trendChartMode: 'bar',
      rankChartMode: 'bar',
      chartInstances: {},
      rankChartRefs: {},
      users: [],
      userForm: { id: null, username: '', password: '', role: 'user', enabled: true },
      audit: { items: [], total: 0 },
      restoringUrl: false,
    }
  },
  computed: {
    tabs() {
      const tabs = [
        { id: 'logs', label: '错误处理', hint: '按类型处理错误' },
        { id: 'errorStats', label: '错误统计', hint: '错误趋势与来源排行' },
      ]
      if (this.me.is_admin) {
        tabs.push({ id: 'clientStats', label: '新增用户统计', hint: '客户端首次上报分析' })
        tabs.push({ id: 'users', label: '用户管理', hint: '账号与角色' })
        tabs.push({ id: 'audit', label: '操作审计', hint: '后台动作记录' })
        tabs.push({ id: 'settings', label: '后台设置', hint: '类型映射与后台配置' })
      }
      return tabs
    },
    currentTab() {
      return this.tabs.find((tab) => tab.id === this.active) || this.tabs[0]
    },
    selectedLogTypeItem() {
      return this.logTypes.find((item) => item.log_type === this.logType) || null
    },
    selectedStatsLogTypeItem() {
      return this.logTypes.find((item) => item.log_type === this.statsLogType) || null
    },
    selectedClientTypeItem() {
      return this.clientTypes.find((item) => item.name === this.statsClientType) || null
    },
    isStatsActive() {
      return this.active === 'errorStats' || this.active === 'clientStats'
    },
    isErrorStats() {
      return this.active === 'errorStats'
    },
    isClientStats() {
      return this.active === 'clientStats'
    },
    statSections() {
      if (!this.stats) return []
      const sourceKeys = ['top_versions', 'top_packages', 'top_users', 'top_ips']
      return Object.entries(this.stats)
        .filter(([key, value]) => {
          if (!Array.isArray(value) || ['trend', 'by_type', 'by_cli_type'].includes(key)) return false
          if (this.isErrorStats && this.errorStatsView === 'overview') return false
          if (this.isErrorStats && this.errorStatsView === 'sources') return sourceKeys.includes(key)
          if (this.isClientStats && this.clientStatsView === 'overview') return false
          return !sourceKeys.includes(key)
        })
        .map(([key, value]) => ({ key, title: this.statTitle(key), value }))
    },
    trendTotal() {
      if (this.stats?.total_in_range != null) return Number(this.stats.total_in_range || 0)
      return (this.stats?.trend || []).reduce((total, item) => total + Number(item.count || 0), 0)
    },
    statsRangeLabel() {
      return {
        '7d': '最近7天',
        '30d': '最近30天',
        '90d': '最近90天',
        '12m': '最近12个月',
        all: '全部时间',
      }[this.statsRange] || '当前范围'
    },
  },
  watch: {
    trendChartMode() {
      this.renderStatsCharts()
    },
    rankChartMode() {
      this.renderStatsCharts()
    },
  },
  async mounted() {
    await this.loadMe()
    window.addEventListener('popstate', this.restoreFromUrl)
    window.addEventListener('resize', this.resizeCharts)
    if (this.me.authenticated) await this.restoreFromUrl()
  },
  beforeUnmount() {
    window.removeEventListener('popstate', this.restoreFromUrl)
    window.removeEventListener('resize', this.resizeCharts)
    Object.values(this.chartInstances).forEach((chart) => chart.dispose())
  },
  methods: {
    async run(task) {
      this.loading = true
      this.error = ''
      try {
        return await task()
      } catch (err) {
        this.error = String(err.message || err)
      } finally {
        this.loading = false
      }
    },
    async loadMe() {
      this.me = await api('/api/auth/me')
    },
    async login() {
      await this.run(async () => {
        this.me = await api('/api/auth/login', {
          method: 'POST',
          body: JSON.stringify(this.loginForm),
        })
        await this.restoreFromUrl()
      })
    },
    async logout() {
      await api('/api/auth/logout', { method: 'POST', body: '{}' })
      this.me = { authenticated: false, is_admin: false }
      this.selectedLog = null
      this.stats = null
      this.updateUrl({}, true)
    },
    async loadLogs() {
      if (!this.logType) return
      await this.run(async () => {
        this.logs = await api('/api/log_list', {
          method: 'POST',
          body: JSON.stringify({
            page: this.page,
            page_size: this.pageSize,
            log_type: this.logType,
            sort_by: this.sortBy,
            sort_order: this.sortOrder,
            status_filter: this.statusFilter,
          }),
        })
        if (this.logs.total_pages > 0 && this.page > this.logs.total_pages) {
          this.page = this.logs.total_pages
          await this.loadLogs()
          return
        }
        this.syncUrl()
      })
    },
    async goLogPage(page) {
      const totalPages = this.logs.total_pages || 1
      this.page = Math.min(Math.max(page, 1), totalPages)
      this.selectedLog = null
      this.userLog = null
      await this.loadLogs()
    },
    async changePageSize() {
      this.page = 1
      this.selectedLog = null
      this.userLog = null
      await this.loadLogs()
    },
    async setStatusFilter(value) {
      this.statusFilter = value
      this.page = 1
      this.selectedLog = null
      this.userLog = null
      await this.loadLogs()
    },
    async setSort(field) {
      if (this.sortBy === field) {
        this.sortOrder = this.sortOrder === 'desc' ? 'asc' : 'desc'
      } else {
        this.sortBy = field
        this.sortOrder = 'desc'
      }
      this.page = 1
      this.selectedLog = null
      this.userLog = null
      await this.loadLogs()
    },
    async loadLogTypes() {
      await this.run(async () => {
        const data = await api('/api/log_types', { method: 'POST', body: '{}' })
        this.logTypes = data.items || []
        if (!this.logType && this.logTypes.length === 1) {
          await this.selectLogType(this.logTypes[0])
        }
      })
    },
    async selectLogType(item) {
      if (!item) return
      this.logType = item.log_type
      this.logTypeForm = {
        log_type: item.log_type,
        display_name: item.display_name,
        enabled: item.enabled,
      }
      this.selectedLog = null
      this.userLog = null
      this.page = 1
      this.statusFilter = 'all'
      this.sortBy = 'last_time'
      this.sortOrder = 'desc'
      await this.loadLogs()
    },
    editLogType(item) {
      this.logTypeForm = {
        log_type: item.log_type,
        display_name: item.display_name,
        enabled: item.enabled,
      }
    },
    newLogType() {
      this.logTypeForm = { log_type: '', display_name: '', enabled: true }
    },
    async saveLogType() {
      await this.run(async () => {
        await api('/api/admin/log_types/save', {
          method: 'POST',
          body: JSON.stringify(this.logTypeForm),
        })
        await this.loadLogTypes()
        const selected = this.logTypes.find((item) => item.log_type === this.logType)
        if (selected) await this.selectLogType(selected)
      })
    },
    async openLog(log) {
      await this.run(async () => {
        this.selectedLog = await api('/api/log_content', {
          method: 'POST',
          body: JSON.stringify({ hash: log.hash }),
        })
        this.userLog = null
        this.syncUrl()
      })
    },
    async completeLog() {
      if (!this.selectedLog) return
      await this.run(async () => {
        await api('/api/log_complete', {
          method: 'POST',
          body: JSON.stringify({ hash: this.selectedLog.hash }),
        })
        await this.openLog(this.selectedLog)
        await this.loadLogs()
      })
    },
    async removeLog() {
      if (!this.selectedLog) return
      await this.run(async () => {
        await api('/api/log_remove', {
          method: 'POST',
          body: JSON.stringify({ hash: this.selectedLog.hash }),
        })
        this.selectedLog = null
        await this.loadLogs()
      })
    },
    async showUserLog(id) {
      this.userLog = await api('/api/user_log', { method: 'POST', body: JSON.stringify({ id }) })
    },
    closeUserLog() {
      this.userLog = null
    },
    async loadStats() {
      const requestSeq = this.statsRequestSeq + 1
      this.statsRequestSeq = requestSeq
      this.statsMode = this.isClientStats ? 'clients' : 'errors'
      this.loading = true
      this.statsLoading = true
      this.statsSourceLoading = false
      this.error = ''
      try {
        if (this.isErrorStats) await this.ensureStatsLogType()
        if (this.isClientStats) await this.ensureClientTypes()
        if (this.isErrorStats && this.errorStatsView === 'sources') {
          const sourceParams = new URLSearchParams({ range: this.statsRange, source_kind: this.errorSourceKind })
          if (this.statsLogType) sourceParams.set('log_type', this.statsLogType)
          const sourceStats = await api(`/api/admin/stats/error_sources?${sourceParams.toString()}`)
          if (requestSeq !== this.statsRequestSeq) return
          this.stats = sourceStats
          this.renderStatsCharts()
          return
        }
        if (this.isClientStats && this.clientStatsView === 'breakdown') {
          const breakdownParams = new URLSearchParams({ range: this.statsRange, client_kind: this.clientBreakdownKind })
          if (this.statsClientType) breakdownParams.set('cli_type', this.statsClientType)
          const breakdownStats = await api(`/api/admin/stats/client_breakdown?${breakdownParams.toString()}`)
          if (requestSeq !== this.statsRequestSeq) return
          this.stats = breakdownStats
          this.renderStatsCharts()
          return
        }
        const endpoint = this.isErrorStats ? '/api/admin/stats/errors' : '/api/admin/stats/clients'
        const params = new URLSearchParams({ granularity: this.granularity, range: this.statsRange })
        if (this.isErrorStats && this.statsLogType) {
          params.set('log_type', this.statsLogType)
        }
        if (this.isClientStats && this.statsClientType) {
          params.set('cli_type', this.statsClientType)
        }
        const stats = await api(`${endpoint}?${params.toString()}`)
        if (requestSeq !== this.statsRequestSeq) return
        this.stats = stats
        this.renderStatsCharts()
      } catch (err) {
        this.error = String(err.message || err)
      } finally {
        if (requestSeq === this.statsRequestSeq) {
          this.statsLoading = false
          this.loading = false
        }
      }
    },
    async loadErrorSourceStats(requestSeq, params) {
      this.statsSourceLoading = true
      try {
        const sourceParams = new URLSearchParams(params)
        sourceParams.delete('granularity')
        sourceParams.set('source_kind', this.errorSourceKind)
        const sourceStats = await api(`/api/admin/stats/error_sources?${sourceParams.toString()}`)
        if (requestSeq !== this.statsRequestSeq || !this.stats) return
        this.stats = { ...this.stats, ...sourceStats }
        this.$nextTick(() => {
          this.statSections.forEach((section) => this.renderRankChart(section))
          this.resizeCharts()
        })
      } catch (err) {
        if (requestSeq === this.statsRequestSeq) {
          this.error = String(err.message || err)
        }
      } finally {
        if (requestSeq === this.statsRequestSeq) {
          this.statsSourceLoading = false
        }
      }
    },
    async ensureStatsLogType() {
      if (!this.isErrorStats) return
      if (!this.logTypes.length) {
        const data = await api('/api/log_types', { method: 'POST', body: '{}' })
        this.logTypes = data.items || []
      }
      if (!this.statsLogTypeInitialized && this.logTypes.length) {
        this.statsLogType = this.logTypes[0].log_type
        this.statsLogTypeInitialized = true
      }
    },
    async ensureClientTypes() {
      if (!this.isClientStats || !this.me.is_admin) return
      if (!this.clientTypes.length) {
        const data = await api('/api/admin/stats/client_types')
        this.clientTypes = data.items || []
      }
    },
    async selectStatsLogType(value) {
      this.statsLogType = value
      this.statsLogTypeInitialized = true
      await this.loadStats()
    },
    async setErrorStatsView(value) {
      this.errorStatsView = value
      await this.loadStats()
    },
    async setErrorSourceKind(value) {
      this.errorSourceKind = value
      await this.loadStats()
    },
    async selectClientType(value) {
      this.statsClientType = value
      await this.loadStats()
    },
    async setClientStatsView(value) {
      this.clientStatsView = value
      await this.loadStats()
    },
    async setClientBreakdownKind(value) {
      this.clientBreakdownKind = value
      await this.loadStats()
    },
    changeStatsRange(value) {
      this.statsRange = value
      if (value === '12m' || value === 'all') {
        this.granularity = 'month'
      }
      this.loadStats()
    },
    changeGranularity(value) {
      this.granularity = value
      this.loadStats()
    },
    async loadUsers() {
      await this.run(async () => {
        const data = await api('/api/admin/users')
        this.users = data.items
      })
    },
    editUser(user) {
      this.userForm = { ...user, password: '' }
    },
    newUser() {
      this.userForm = { id: null, username: '', password: '', role: 'user', enabled: true }
    },
    async saveUser() {
      await this.run(async () => {
        await api('/api/admin/users/save', { method: 'POST', body: JSON.stringify(this.userForm) })
        this.newUser()
        await this.loadUsers()
      })
    },
    async loadAudit() {
      await this.run(async () => {
        this.audit = await api('/api/admin/audit_logs?page=1&page_size=50')
      })
    },
    switchTab(tab) {
      this.active = tab
      if (tab === 'logs') this.loadLogTypes()
      if (tab === 'errorStats' || tab === 'clientStats') this.loadStats()
      if (tab === 'users') this.loadUsers()
      if (tab === 'audit') this.loadAudit()
      if (tab === 'settings') this.loadLogTypes()
      this.syncUrl()
    },
    fmt(ts) {
      if (!ts) return ''
      return new Date(ts * 1000).toLocaleString()
    },
    statusText(status) {
      return status === 1 ? '已处理' : status === -1 ? '复发' : '待处理'
    },
    statusClass(status) {
      return status === 1 ? 'ok' : status === -1 ? 'warn' : 'hot'
    },
    roleText(role) {
      return role === 'admin' ? '管理员' : '普通用户'
    },
    statTitle(key) {
      const titles = {
        by_type: '错误类型',
        by_status: '状态分布',
        top_versions: '版本排行',
        top_packages: '包名排行',
        top_users: '用户排行',
        top_ips: 'IP排行',
        by_cli_type: '客户端类型',
        by_region: '地区分布',
        by_package: this.isClientStats ? '新增包名分布' : '包名分布',
        by_user: '用户分布',
      }
      return titles[key] || key
    },
    statusName(name) {
      const value = String(name)
      if (value === '1') return '已处理'
      if (value === '-1') return '复发'
      if (value === '0') return '待处理'
      return value
    },
    barWidth(count, list) {
      const max = Math.max(...list.map((item) => item.count), 1)
      return `${Math.max((count / max) * 100, 4)}%`
    },
    shortHash(hash) {
      return hash ? hash.slice(0, 8) : ''
    },
    backToLogTypes() {
      this.logType = ''
      this.selectedLog = null
      this.userLog = null
      this.logs = { items: [], total: 0, pending: 0, solved: 0, total_pages: 0 }
      this.syncUrl()
    },
    sortLabel(field) {
      if (this.sortBy !== field) return ''
      return this.sortOrder === 'desc' ? ' ↓' : ' ↑'
    },
    syncUrl() {
      if (this.restoringUrl) return
      const params = { view: this.active }
      if (this.active === 'logs') {
        if (this.logType) {
          params.log_type = this.logType
          params.page = String(this.page)
          params.page_size = String(this.pageSize)
          params.status = this.statusFilter
          params.sort_by = this.sortBy
          params.sort_order = this.sortOrder
        }
        if (this.selectedLog?.hash) params.hash = this.selectedLog.hash
      }
      this.updateUrl(params)
    },
    updateUrl(params, replace = false) {
      const query = new URLSearchParams(params)
      const next = `${window.location.pathname}${query.toString() ? `?${query.toString()}` : ''}`
      const current = `${window.location.pathname}${window.location.search}`
      if (next === current) return
      window.history[replace ? 'replaceState' : 'pushState']({}, '', next)
    },
    async restoreFromUrl() {
      if (!this.me.authenticated) return
      this.restoringUrl = true
      try {
        const params = new URLSearchParams(window.location.search)
        const rawView = params.get('view') || 'logs'
        const view = rawView === 'stats' ? 'errorStats' : rawView
        this.active = this.tabs.some((tab) => tab.id === view) ? view : 'logs'

        if (this.active === 'logs') {
          await this.loadLogTypes()
          const logType = params.get('log_type') || ''
          if (logType) {
            this.logType = logType
            const item = this.logTypes.find((type) => type.log_type === logType)
            this.logTypeForm = {
              log_type: logType,
              display_name: item?.display_name || logType,
              enabled: item?.enabled ?? true,
            }
            this.page = Math.max(Number(params.get('page') || 1), 1)
            this.pageSize = Math.min(Math.max(Number(params.get('page_size') || 20), 1), 100)
            this.statusFilter = params.get('status') || 'all'
            this.sortBy = params.get('sort_by') || 'last_time'
            this.sortOrder = params.get('sort_order') || 'desc'
            await this.loadLogs()
            const hash = params.get('hash')
            if (hash) await this.openLog({ hash })
          } else {
            this.backToLogTypes()
          }
        }

        if (this.active === 'errorStats' || this.active === 'clientStats') await this.loadStats()
        if (this.active === 'users') await this.loadUsers()
        if (this.active === 'audit') await this.loadAudit()
        if (this.active === 'settings') await this.loadLogTypes()
      } finally {
        this.restoringUrl = false
      }
    },
    setRankChartRef(key, el) {
      if (el) {
        this.rankChartRefs[key] = el
        return
      }
      delete this.rankChartRefs[key]
      const chart = this.chartInstances[`rank-${key}`]
      if (chart && !chart.isDisposed?.()) chart.dispose()
      delete this.chartInstances[`rank-${key}`]
    },
    renderStatsCharts() {
      this.$nextTick(() => {
        if (!this.stats) return
        if (this.$refs.trendChart) this.renderTrendChart()
        this.statSections.forEach((section) => this.renderRankChart(section))
      })
    },
    getChart(key, el) {
      if (!el) return null
      const current = this.chartInstances[key]
      if (current && !current.isDisposed?.() && current.getDom?.() !== el) {
        current.dispose()
        delete this.chartInstances[key]
      }
      if (!this.chartInstances[key] || this.chartInstances[key].isDisposed?.()) {
        this.chartInstances[key] = init(el)
      }
      return this.chartInstances[key]
    },
    renderTrendChart() {
      const chart = this.getChart('trend', this.$refs.trendChart)
      if (!chart) return
      const data = this.stats.trend || []
      const visibleCount = this.granularity === 'hour' ? 72 : this.granularity === 'day' ? 45 : 24
      const endValue = Math.max(data.length - 1, 0)
      const startValue = Math.max(data.length - visibleCount, 0)
      chart.setOption({
        color: ['#0f766e'],
        tooltip: { trigger: 'axis' },
        grid: { left: 48, right: 24, top: 28, bottom: 56 },
        xAxis: {
          type: 'category',
          data: data.map((item) => item.bucket),
          axisLabel: { color: '#667789', hideOverlap: true },
          axisLine: { lineStyle: { color: '#dbe4ed' } },
        },
        yAxis: {
          type: 'value',
          axisLabel: { color: '#667789' },
          splitLine: { lineStyle: { color: '#e7edf3' } },
        },
        dataZoom: [
          {
            type: 'inside',
            xAxisIndex: 0,
            filterMode: 'none',
            startValue,
            endValue,
            zoomOnMouseWheel: true,
            moveOnMouseMove: true,
            moveOnMouseWheel: false,
          },
        ],
        series: [{
          name: '数量',
          type: this.trendChartMode,
          smooth: this.trendChartMode === 'line',
          data: data.map((item) => item.count),
          barMaxWidth: 34,
          areaStyle: this.trendChartMode === 'line' ? { opacity: 0.12 } : undefined,
        }],
      }, true)
    },
    renderRankChart(section) {
      const chart = this.getChart(`rank-${section.key}`, this.rankChartRefs[section.key])
      if (!chart) return
      const data = section.value || []
      const names = data.map((item) => this.statusName(item.name))
      if (this.rankChartMode === 'pie') {
        chart.setOption({
          color: ['#0f766e', '#2563eb', '#d97706', '#16a34a', '#dc2626', '#7c3aed', '#0891b2', '#64748b'],
          tooltip: { trigger: 'item' },
          legend: { type: 'scroll', bottom: 0, textStyle: { color: '#667789' } },
          series: [{
            name: section.title,
            type: 'pie',
            radius: ['42%', '68%'],
            center: ['50%', '45%'],
            data: data.map((item) => ({ name: this.statusName(item.name), value: item.count })),
          }],
        }, true)
        return
      }
      chart.setOption({
        color: ['#0f766e'],
        tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
        grid: { left: 120, right: 28, top: 16, bottom: 28 },
        xAxis: {
          type: 'value',
          axisLabel: { color: '#667789' },
          splitLine: { lineStyle: { color: '#e7edf3' } },
        },
        yAxis: {
          type: 'category',
          data: names,
          inverse: true,
          axisLabel: { color: '#667789', width: 110, overflow: 'truncate' },
          axisLine: { lineStyle: { color: '#dbe4ed' } },
        },
        series: [{
          name: '数量',
          type: 'bar',
          data: data.map((item) => item.count),
          barMaxWidth: 18,
        }],
      }, true)
    },
    resizeCharts() {
      Object.values(this.chartInstances).forEach((chart) => chart.resize())
    },
  },
}
</script>

<template>
  <main v-if="!me.authenticated" class="login">
    <form class="login-panel" @submit.prevent="login">
      <div class="login-mark">TH</div>
      <h1>错误日志后台</h1>
      <label>
        <span>用户名</span>
        <input v-model="loginForm.username" autocomplete="username" />
      </label>
      <label>
        <span>密码</span>
        <input v-model="loginForm.password" type="password" autocomplete="current-password" />
      </label>
      <button class="primary full" type="submit" :disabled="loading">登录</button>
      <p v-if="error" class="error">{{ error }}</p>
    </form>
  </main>

  <main v-else class="shell">
    <aside class="sidebar">
      <div class="brand">
        <div class="brand-mark">TH</div>
        <div>
          <strong>Tiny HTTP</strong>
          <span>错误日志后台</span>
        </div>
      </div>

      <nav>
        <button
          v-for="tab in tabs"
          :key="tab.id"
          :class="{ active: active === tab.id }"
          @click="switchTab(tab.id)"
        >
          <span>{{ tab.label }}</span>
          <small>{{ tab.hint }}</small>
        </button>
      </nav>

      <div class="account">
        <div class="avatar">{{ me.username?.slice(0, 1)?.toUpperCase() }}</div>
        <div>
          <strong>{{ me.username }}</strong>
          <span>{{ roleText(me.role) }}</span>
        </div>
      </div>
      <button class="ghost" @click="logout">退出</button>
    </aside>

    <section class="workspace">
      <header class="topbar">
        <div>
          <h1>{{ currentTab.label }}</h1>
          <p>{{ currentTab.hint }}</p>
        </div>
        <div class="top-actions">
          <span v-if="loading" class="loading">加载中</span>
          <button v-if="active === 'logs'" @click="loadLogTypes">刷新类型</button>
          <button v-if="isStatsActive" class="primary" @click="loadStats">刷新统计</button>
          <button v-if="active === 'users'" class="primary" @click="newUser">新增用户</button>
        </div>
      </header>

      <p v-if="error" class="error">{{ error }}</p>

      <div v-if="active === 'logs' && !logType" class="log-type-screen">
        <section class="type-hero">
          <div>
            <h2>选择日志类型</h2>
            <p>进入对应类型后集中处理错误，未配置映射的类型会显示原始名称。</p>
          </div>
          <button @click="loadLogTypes">刷新类型</button>
        </section>

        <section class="type-grid">
          <button
            v-for="item in logTypes"
            :key="item.log_type"
            class="big-type-card"
            @click="selectLogType(item)"
          >
            <span>{{ item.display_name }}</span>
            <small>{{ item.log_type }}</small>
            <div class="type-metrics">
              <b>{{ item.total }}</b>
              <em>{{ item.pending }} 待处理</em>
              <em>{{ item.solved }} 已处理</em>
            </div>
          </button>
        </section>

        <div v-if="!logTypes.length" class="empty-state">暂无日志类型</div>
      </div>

      <div v-if="active === 'logs' && logType" class="log-workspace">
        <section class="panel log-list-panel">
          <div class="panel-head">
            <div>
              <h2>{{ logs.log_type_name || selectedLogTypeItem?.display_name || '错误列表' }}</h2>
              <p>{{ logType }}</p>
            </div>
            <div class="panel-actions">
              <button @click="backToLogTypes">返回类型</button>
              <button @click="loadLogs">刷新</button>
            </div>
          </div>

          <div class="metric-row filter-metrics">
            <button class="metric hot" :class="{ active: statusFilter === 'all' }" @click="setStatusFilter('all')">
              <span>全部</span><b>{{ logs.total }}</b><em>显示全部错误</em>
            </button>
            <button class="metric warn" :class="{ active: statusFilter === 'pending' }" @click="setStatusFilter('pending')">
              <span>待处理</span><b>{{ logs.pending }}</b><em>只看未解决</em>
            </button>
            <button class="metric ok" :class="{ active: statusFilter === 'solved' }" @click="setStatusFilter('solved')">
              <span>已处理</span><b>{{ logs.solved }}</b><em>只看已解决</em>
            </button>
          </div>

          <div class="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>状态</th>
                  <th><button class="sort-head" @click="setSort('total_count')">次数{{ sortLabel('total_count') }}</button></th>
                  <th><button class="sort-head" @click="setSort('last_time')">最后时间{{ sortLabel('last_time') }}</button></th>
                  <th>处理人</th>
                  <th>消息</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="item in logs.items"
                  :key="item.hash"
                  :class="{ selected: selectedLog?.hash === item.hash }"
                  @click="openLog(item)"
                >
                  <td><span class="status-pill" :class="statusClass(item.status)">{{ statusText(item.status) }}</span></td>
                  <td>{{ item.total_count }}</td>
                  <td>{{ fmt(item.last_time) }}</td>
                  <td>{{ item.resolved_by_username || '' }}</td>
                  <td class="message-cell">{{ item.message }}</td>
                </tr>
              </tbody>
            </table>
          </div>

          <div class="pagination">
            <span>共 {{ logs.filtered_total ?? logs.total }} 条，第 {{ page }} / {{ logs.total_pages || 1 }} 页</span>
            <label>
              每页
              <select v-model.number="pageSize" @change="changePageSize">
                <option :value="20">20</option>
                <option :value="50">50</option>
                <option :value="100">100</option>
              </select>
            </label>
            <button :disabled="page <= 1" @click="goLogPage(1)">首页</button>
            <button :disabled="page <= 1" @click="goLogPage(page - 1)">上一页</button>
            <button :disabled="page >= (logs.total_pages || 1)" @click="goLogPage(page + 1)">下一页</button>
            <button :disabled="page >= (logs.total_pages || 1)" @click="goLogPage(logs.total_pages || 1)">末页</button>
          </div>
        </section>

        <section class="panel detail-panel">
          <div v-if="!selectedLog" class="empty-state">选择一条错误查看详情</div>

          <template v-else>
            <div class="detail-hero">
              <div>
                <span class="status-pill" :class="statusClass(selectedLog.status)">
                  {{ statusText(selectedLog.status) }}
                </span>
                <h2>{{ selectedLog.log_type_name }}</h2>
                <p>{{ selectedLog.log_type }} · {{ shortHash(selectedLog.hash) }}</p>
              </div>
              <div class="detail-actions">
                <button class="primary" @click="completeLog">标记处理</button>
                <button v-if="selectedLog.can_remove" class="danger" @click="removeLog">删除</button>
              </div>
            </div>

            <div class="detail-meta">
              <span>首次 {{ fmt(selectedLog.first_time) }}</span>
              <span>最后 {{ fmt(selectedLog.last_time) }}</span>
              <span>处理人 {{ selectedLog.resolved_by_username || '无' }}</span>
            </div>

            <pre>{{ selectedLog.message }}</pre>

            <div v-if="selectedLog.resolution_history?.length" class="resolution-history">
              <h3>处理历史</h3>
              <div
                v-for="item in selectedLog.resolution_history"
                :key="item.id"
                class="resolution-history-item"
              >
                <span>{{ item.resolved_by_username }}</span>
                <time>{{ fmt(item.resolved_at) }}</time>
              </div>
            </div>

            <h3>上报来源</h3>
            <div class="table-wrap compact">
              <table>
                <thead>
                  <tr><th>用户</th><th>版本</th><th>包名</th><th>IP</th><th>时间</th><th>日志</th></tr>
                </thead>
                <tbody>
                  <tr v-for="user in selectedLog.user_list" :key="user.id">
                    <td>{{ user.user }}</td>
                    <td>{{ user.version }}</td>
                    <td>{{ user.package }}</td>
                    <td>{{ user.ip }}</td>
                    <td>{{ user.time }}</td>
                    <td><button @click="showUserLog(user.id)">查看</button></td>
                  </tr>
                </tbody>
              </table>
            </div>
          </template>
        </section>
      </div>

      <section v-if="isStatsActive" class="stats-screen">
        <div v-if="isErrorStats" class="stats-type-selector">
          <div>
            <span>错误类型</span>
            <strong>{{ selectedStatsLogTypeItem?.display_name || '全部错误类型' }}</strong>
          </div>
          <div class="type-chip-row">
            <button :class="{ active: statsLogType === '' }" @click="selectStatsLogType('')">全部</button>
            <button
              v-for="item in logTypes"
              :key="item.log_type"
              :class="{ active: statsLogType === item.log_type }"
              @click="selectStatsLogType(item.log_type)"
            >
              <span>{{ item.display_name }}</span>
              <small>总计 {{ item.total }}</small>
            </button>
          </div>
        </div>

        <div v-if="isClientStats" class="stats-type-selector">
          <div>
            <span>客户端类型</span>
            <strong>{{ selectedClientTypeItem?.name || '全部客户端' }}</strong>
          </div>
          <div class="type-chip-row">
            <button :class="{ active: statsClientType === '' }" @click="selectClientType('')">全部</button>
            <button
              v-for="item in clientTypes"
              :key="item.name"
              :class="{ active: statsClientType === item.name }"
              @click="selectClientType(item.name)"
            >
              <span>{{ item.name || '未命名类型' }}</span>
              <small>{{ item.count }}</small>
            </button>
          </div>
        </div>

        <div class="section-head">
          <div>
            <h2>{{ isErrorStats ? '错误统计分析' : '新增用户统计' }}</h2>
            <p>{{ isErrorStats ? '错误趋势、状态与来源排行' : '通过客户端首次配置上报估算新增用户' }}</p>
          </div>
          <div class="stats-toolbar">
            <div class="segmented">
              <button :class="{ active: statsRange === '7d' }" @click="changeStatsRange('7d')">7天</button>
              <button :class="{ active: statsRange === '30d' }" @click="changeStatsRange('30d')">30天</button>
              <button :class="{ active: statsRange === '90d' }" @click="changeStatsRange('90d')">90天</button>
              <button :class="{ active: statsRange === '12m' }" @click="changeStatsRange('12m')">12个月</button>
              <button :class="{ active: statsRange === 'all' }" @click="changeStatsRange('all')">全部</button>
            </div>
            <label class="inline-select">
              趋势
              <select v-model="trendChartMode">
                <option value="bar">柱状图</option>
                <option value="line">折线图</option>
              </select>
            </label>
            <label class="inline-select">
              分布
              <select v-model="rankChartMode">
                <option value="bar">条形图</option>
                <option value="pie">环形图</option>
              </select>
            </label>
          </div>
        </div>

        <div v-if="isErrorStats" class="stats-mode-row">
          <div class="segmented">
            <button :class="{ active: errorStatsView === 'overview' }" @click="setErrorStatsView('overview')">趋势状态</button>
            <button :class="{ active: errorStatsView === 'sources' }" @click="setErrorStatsView('sources')">来源排行</button>
          </div>
          <div v-if="errorStatsView === 'sources'" class="segmented">
            <button :class="{ active: errorSourceKind === 'package' }" @click="setErrorSourceKind('package')">包名</button>
            <button :class="{ active: errorSourceKind === 'user' }" @click="setErrorSourceKind('user')">用户</button>
            <button :class="{ active: errorSourceKind === 'ip' }" @click="setErrorSourceKind('ip')">IP</button>
            <button :class="{ active: errorSourceKind === 'version' }" @click="setErrorSourceKind('version')">版本</button>
          </div>
        </div>

        <div v-if="isClientStats" class="stats-mode-row">
          <div class="segmented">
            <button :class="{ active: clientStatsView === 'overview' }" @click="setClientStatsView('overview')">新增趋势</button>
            <button :class="{ active: clientStatsView === 'breakdown' }" @click="setClientStatsView('breakdown')">分布分析</button>
          </div>
          <div v-if="clientStatsView === 'breakdown'" class="segmented">
            <button :class="{ active: clientBreakdownKind === 'region' }" @click="setClientBreakdownKind('region')">地区</button>
            <button :class="{ active: clientBreakdownKind === 'package' }" @click="setClientBreakdownKind('package')">包名</button>
          </div>
        </div>

        <div class="stats-content">
          <div v-if="statsLoading" class="stats-loading">
            <div>正在加载最新统计数据</div>
          </div>

          <template v-if="stats">
            <div
              v-if="(isErrorStats && errorStatsView === 'overview') || (isClientStats && clientStatsView === 'overview')"
              class="trend-panel"
            >
              <div class="chart-head">
                <div>
                  <h3>趋势</h3>
                  <p>{{ stats.granularity === 'hour' ? '按小时聚合' : stats.granularity === 'month' ? '按月聚合' : '按日聚合' }}</p>
                </div>
                <div class="segmented trend-granularity">
                  <button :class="{ active: granularity === 'hour' }" @click="changeGranularity('hour')">小时</button>
                  <button :class="{ active: granularity === 'day' }" @click="changeGranularity('day')">日</button>
                  <button :class="{ active: granularity === 'month' }" @click="changeGranularity('month')">月</button>
                </div>
              </div>
              <div ref="trendChart" class="echart trend-echart"></div>
              <div class="trend-summary">
                <span>{{ stats.trend?.length || 0 }} 个时间点</span>
                <b>{{ statsRangeLabel }} {{ trendTotal }}</b>
              </div>
            </div>

            <div v-if="statsSourceLoading" class="source-loading">来源排行正在后台加载</div>

            <div class="stats-grid">
              <div v-for="section in statSections" :key="section.key" class="rank-panel">
                <div class="rank-head">
                  <h3>{{ section.title }}</h3>
                  <span>{{ section.value.length }} 项</span>
                </div>
                <div :ref="(el) => setRankChartRef(section.key, el)" class="echart rank-echart"></div>
                <div v-if="section.value.length" class="rank-list">
                  <div v-for="item in section.value.slice(0, 5)" :key="`${section.key}-${item.name}`">
                    <span>{{ statusName(item.name) }}</span>
                    <b>{{ item.count }}</b>
                  </div>
                </div>
                <div v-else class="mini-empty">暂无数据</div>
              </div>
            </div>
          </template>
        </div>
      </section>

      <div v-if="active === 'users'" class="manage-layout">
        <section class="panel">
          <div class="panel-head">
            <div>
              <h2>后台用户</h2>
              <p>{{ users.length }} 个账号</p>
            </div>
            <button class="primary" @click="newUser">新增</button>
          </div>
          <div class="table-wrap">
            <table>
              <thead>
                <tr><th>用户名</th><th>角色</th><th>状态</th><th>最后登录</th></tr>
              </thead>
              <tbody>
                <tr v-for="user in users" :key="user.id" @click="editUser(user)">
                  <td>{{ user.username }}</td>
                  <td>{{ roleText(user.role) }}</td>
                  <td><span class="status-pill" :class="user.enabled ? 'ok' : 'muted'">{{ user.enabled ? '启用' : '禁用' }}</span></td>
                  <td>{{ fmt(user.last_login_at) }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>

        <form class="panel side-form" @submit.prevent="saveUser">
          <div class="panel-head">
            <div>
              <h2>{{ userForm.id ? '编辑用户' : '新增用户' }}</h2>
              <p>{{ userForm.username || '未命名账号' }}</p>
            </div>
          </div>
          <label><span>用户名</span><input v-model="userForm.username" /></label>
          <label><span>密码</span><input v-model="userForm.password" type="password" placeholder="留空则不修改" /></label>
          <label>
            <span>角色</span>
            <select v-model="userForm.role">
              <option value="user">普通用户</option>
              <option value="admin">管理员</option>
            </select>
          </label>
          <label class="checkline"><input v-model="userForm.enabled" type="checkbox" /><span>启用账号</span></label>
          <button class="primary full" type="submit">保存用户</button>
        </form>
      </div>

      <section v-if="active === 'audit'" class="panel">
        <div class="panel-head">
          <div>
            <h2>操作审计</h2>
            <p>{{ audit.total }} 条记录</p>
          </div>
          <button @click="loadAudit">刷新</button>
        </div>
        <div class="table-wrap">
          <table>
            <thead>
              <tr><th>时间</th><th>用户</th><th>动作</th><th>目标</th><th>结果</th><th>IP</th></tr>
            </thead>
            <tbody>
              <tr v-for="item in audit.items" :key="item.id">
                <td>{{ fmt(item.created_at) }}</td>
                <td>{{ item.actor_username }}</td>
                <td>{{ item.action }}</td>
                <td>{{ item.target_type }} {{ item.target_id }}</td>
                <td><span class="status-pill" :class="item.result === 'success' ? 'ok' : 'hot'">{{ item.result }}</span></td>
                <td>{{ item.ip }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <div v-if="active === 'settings'" class="manage-layout">
        <section class="panel">
          <div class="panel-head">
            <div>
              <h2>日志类型映射</h2>
              <p>把原始 log_type 显示成业务名称</p>
            </div>
            <button @click="loadLogTypes">刷新</button>
          </div>
          <div class="table-wrap">
            <table>
              <thead>
                <tr><th>显示名称</th><th>原始类型</th><th>总数</th><th>待处理</th><th>状态</th></tr>
              </thead>
              <tbody>
                <tr v-for="item in logTypes" :key="item.log_type" @click="editLogType(item)">
                  <td>{{ item.display_name }}</td>
                  <td>{{ item.log_type }}</td>
                  <td>{{ item.total }}</td>
                  <td>{{ item.pending }}</td>
                  <td>
                    <span class="status-pill" :class="item.enabled ? 'ok' : 'muted'">
                      {{ item.enabled ? '启用' : '禁用' }}
                    </span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>

        <form class="panel side-form" @submit.prevent="saveLogType">
          <div class="panel-head">
            <div>
              <h2>类型映射</h2>
              <p>{{ logTypeForm.log_type || '新建映射' }}</p>
            </div>
            <button type="button" @click="newLogType">新建</button>
          </div>
          <label>
            <span>原始类型</span>
            <input v-model="logTypeForm.log_type" placeholder="error_999" />
          </label>
          <label>
            <span>显示名称</span>
            <input v-model="logTypeForm.display_name" placeholder="999平台" />
          </label>
          <label class="checkline">
            <input v-model="logTypeForm.enabled" type="checkbox" />
            <span>启用映射</span>
          </label>
          <button class="primary full" type="submit">保存映射</button>
        </form>
      </div>

      <div v-if="userLog" class="log-modal" @click.self="closeUserLog">
        <section class="log-modal-panel">
          <header>
            <div>
              <span>上报来源日志</span>
              <h2>#{{ userLog.id }}</h2>
            </div>
            <button @click="closeUserLog">关闭</button>
          </header>
          <pre>{{ userLog.logs }}</pre>
        </section>
      </div>
    </section>
  </main>
</template>
