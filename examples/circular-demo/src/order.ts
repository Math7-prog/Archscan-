import { getInvoice } from './invoice';

export function getOrders(userId: string) {
  return [{ id: '1', invoice: getInvoice('1') }];
}
