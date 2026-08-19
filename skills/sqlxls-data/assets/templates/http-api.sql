-- HTTP JSON。Token 用环境变量，不要写进文件。
--   export SQLXLS_BEARER_TOKEN=...
--   或 headers 里使用 ${TOKEN}
LOAD orders FROM 'https://api.example.com/orders' WITH (
  format='json',
  json_path='data'
);

SELECT status, COUNT(*) AS n, SUM(CAST(amount AS REAL)) AS total
FROM orders
GROUP BY status
ORDER BY n DESC;
