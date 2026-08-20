-- 相似源扇出成一张表。用法：
--   sqlxls dynamic-fanout.sql --set base=https://api.example.com --strict
SET base = 'https://api.example.com';

LOAD orders FROM '${base}/${region}/orders' WITH (
  format='json',
  json_path='data',
  page_param='page',
  page_to=20
)
FOR region IN ('east', 'west');

-- 按日扇出示例（按需改 locator）：
-- LOAD daily FROM '${base}/orders?dt=${d}' WITH (format='json', json_path='data')
-- FOR d IN DATE '2024-01-01'..'2024-01-31';

SELECT _region, COUNT(*) AS n
FROM orders
GROUP BY _region
ORDER BY _region;
