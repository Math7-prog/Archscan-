import { getUser } from './user';

export function getInvoice(orderId: string) {
  return { orderId, user: getUser('unknown') };
}
