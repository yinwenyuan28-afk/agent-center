/**
 * 类名合并工具。
 *
 * shadcn 的标准做法：clsx 处理条件类名，tailwind-merge 消解冲突
 * （比如同时传 `p-2` 和 `p-4` 时后者生效，而不是两个都留着）。
 */
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
