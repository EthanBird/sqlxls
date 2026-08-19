-- 多源关联后导出：bash scripts/run.sh -o result.xlsx assets/templates/join-and-aggregate.sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD orders FROM 'orders.csv';

SELECT
  u.id,
  u.name,
  COUNT(o.id) AS order_n,
  SUM(CAST(o.amount AS REAL)) AS total
FROM users u
LEFT JOIN orders o ON CAST(u.id AS TEXT) = CAST(o.user_id AS TEXT)
GROUP BY u.id, u.name
ORDER BY total DESC;
