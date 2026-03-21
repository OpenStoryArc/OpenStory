/** Horizontal bar chart of tool usage distribution (recharts). */

import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Cell,
  ResponsiveContainer,
  Tooltip,
} from "recharts";
import { prepareToolChartData } from "@/lib/tool-chart-data";
import { toolColor } from "@/lib/tool-colors";

interface ToolChartProps {
  breakdown: Readonly<Record<string, number>>;
  maxBars?: number;
}

export function ToolChart({ breakdown, maxBars = 8 }: ToolChartProps) {
  const data = prepareToolChartData(breakdown, maxBars);
  if (data.length === 0) return null;

  return (
    <div data-testid="tool-chart">
      <h3 className="text-xs text-[#565f89] mb-2">Tool Distribution</h3>
      <ResponsiveContainer width="100%" height={data.length * 28 + 16}>
        <BarChart
          data={data}
          layout="vertical"
          margin={{ top: 0, right: 8, bottom: 0, left: 0 }}
        >
          <XAxis
            type="number"
            tick={{ fill: "#565f89", fontSize: 10 }}
            axisLine={{ stroke: "#2f3348" }}
            tickLine={false}
          />
          <YAxis
            type="category"
            dataKey="name"
            width={64}
            tick={{ fill: "#c0caf5", fontSize: 11 }}
            axisLine={false}
            tickLine={false}
          />
          <Tooltip
            contentStyle={{
              backgroundColor: "#1a1b26",
              border: "1px solid #2f3348",
              borderRadius: 4,
              fontSize: 12,
              color: "#c0caf5",
            }}
            cursor={{ fill: "#7aa2f710" }}
          />
          <Bar dataKey="count" radius={[0, 2, 2, 0]} maxBarSize={20}>
            {data.map((entry) => (
              <Cell key={entry.name} fill={toolColor(entry.name)} />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}
