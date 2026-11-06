import { mount } from '@vue/test-utils';
import { expect, test } from 'vitest';
import App from '../src/App.vue';

test('実行前は件数・marker・SQLを未取得として表示する', () => {
  const wrapper = mount(App);
  expect(wrapper.findAll('.big b').map(item => item.text())).toEqual(['—', '—']);
  expect(wrapper.findAll('.marker strong').map(item => item.text())).toEqual(['—', '—']);
  expect(wrapper.text()).toContain('処理の計画は未取得です');
  expect(wrapper.find('pre').exists()).toBe(false);
  expect(wrapper.get('.primary').attributes('disabled')).toBeDefined();
});
