import { getOrders } from './order';

export function getUser(id: string) {
  return { id, orders: getOrders(id) };
}
